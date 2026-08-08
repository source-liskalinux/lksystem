//! Drives `.device` units from kernel uevents (see `crate::platform::netlink_uevent`
//! for where those are actually read off the wire). This module owns:
//!
//! 1. `start_device_events_thread` -- a background thread that listens forever and
//!    flips `DeviceState.found` (and the unit's `UnitStatus`) whenever the kernel
//!    reports a device appearing or disappearing. If no unit with the computed name
//!    exists yet (the common case -- most devices have no reason to have a `.device`
//!    file of their own), one is synthesized on the fly, exactly like systemd does.
//! 2. `coldplug` -- devices that were already attached *before* lksystem started
//!    listening (basically everything detected during kernel boot) never generate a
//!    uevent on their own after the fact. Writing `"add"` to their sysfs `uevent`
//!    file asks the kernel to resend one, which is the same trick udev/systemd use.
//!
//! Only Linux has `NETLINK_KOBJECT_UEVENT`, so this whole module is a no-op stub on
//! other platforms (mirroring how `platform::netlink_uevent` itself is gated).

use crate::runtime_info::ArcMutRuntimeInfo;
use crate::units::*;
use crate::ui;

#[cfg(target_os = "linux")]
pub fn start_device_events_thread(run_info: ArcMutRuntimeInfo) {
    use crate::platform::{spawn_uevent_listener, UeventMessage};

    let spawn_result = spawn_uevent_listener(move |msg: UeventMessage| {
        handle_uevent(run_info.clone(), msg);
    });

    if let Err(e) = spawn_result {
        // Not fatal: lksystem can still boot without device tracking, it just means
        // .mount/.device dependants will never see their device unit reach
        // "Started" on their own. Loudly warn instead of silently degrading.
        ui::warning(format!(
            "Could not start the netlink uevent listener, .device units will never \
             become ready: {}",
            e
        ));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn start_device_events_thread(_run_info: ArcMutRuntimeInfo) {
    ui::warning(format!("Device unit tracking (netlink uevents) is only supported on Linux"));
}

/// Walks the canonical kernel device tree and asks every device to resend its
/// uevent, so devices that were already present before we started listening (i.e.
/// essentially everything that was detected during early boot) still end up
/// reflected in `.device` unit state. Must be called *after*
/// `start_device_events_thread`, otherwise the resent events have nobody to catch
/// them.
#[cfg(target_os = "linux")]
pub fn coldplug() {
    coldplug_dir(&std::path::PathBuf::from("/sys/devices"));
}

#[cfg(not(target_os = "linux"))]
pub fn coldplug() {}

#[cfg(target_os = "linux")]
fn coldplug_dir(dir: &std::path::Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Common and harmless: permission denied on some sysfs nodes, or the
        // device vanished between us listing the parent dir and descending into
        // it. Neither should ever be allowed to take PID 1 down.
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }

        let uevent_path = path.join("uevent");
        if uevent_path.is_file() {
            if let Err(e) = std::fs::write(&uevent_path, b"add") {
                ui::log(format!("Coldplug: could not trigger {:?}: {}", uevent_path, e));
            }
        }

        coldplug_dir(&path);
    }
}

#[cfg(target_os = "linux")]
fn handle_uevent(run_info: ArcMutRuntimeInfo, msg: crate::platform::UeventMessage) {
    use crate::platform::{
        device_unit_name_from_devname, device_unit_name_from_devpath, UeventAction,
    };

    let now_present = match msg.action {
        UeventAction::Add
        | UeventAction::Online
        | UeventAction::Bind
        | UeventAction::Change
        | UeventAction::Move => true,
        UeventAction::Remove | UeventAction::Offline | UeventAction::Unbind => false,
    };

    let primary_name = msg.devname.as_deref().map(device_unit_name_from_devname);
    let fallback_name = device_unit_name_from_devpath(&msg.devpath);

    let unit_id = {
        let run_info_locked = run_info.read().unwrap();
        if let Some(name) = &primary_name {
            let id = UnitId {
                kind: UnitIdKind::Device,
                name: name.clone(),
            };
            if run_info_locked.unit_table.contains_key(&id) {
                Some(id)
            } else {
                None
            }
        } else {
            None
        }
    };
    let unit_id = unit_id.or_else(|| {
        let run_info_locked = run_info.read().unwrap();
        let id = UnitId {
            kind: UnitIdKind::Device,
            name: fallback_name.clone(),
        };
        if run_info_locked.unit_table.contains_key(&id) {
            Some(id)
        } else {
            None
        }
    });

    let unit_id = match unit_id {
        Some(id) => id,
        None => {
            if !now_present {
                // Never seen this device before and it's now gone -- nothing to
                // record.
                return;
            }
            let id = UnitId {
                kind: UnitIdKind::Device,
                name: primary_name.unwrap_or(fallback_name),
            };
            ui::log(format!("Synthesizing new device unit: {}", id.name));
            let unit = Unit {
                common: Common {
                    status: std::sync::RwLock::new(UnitStatus::NeverStarted),
                    unit: UnitConfig {
                        description: format!("Device {}", msg.devpath),
                        refs_by_name: Vec::new(),
                        conflicts: Vec::new(),
                    },
                    conditions: ParsedConditions::default(),
                    dependencies: Dependencies {
                        wants: Vec::new(),
                        wanted_by: Vec::new(),
                        requires: Vec::new(),
                        required_by: Vec::new(),
                        before: Vec::new(),
                        after: Vec::new(),
                    },
                },
                specific: Specific::Device(DeviceSpecific {
                    state: std::sync::RwLock::new(DeviceState {
                        common: CommonState::default(),
                        found: false,
                    }),
                }),
                id: id.clone(),
            };
            let mut run_info_locked = run_info.write().unwrap();
            // Someone may have raced us and inserted/loaded this unit between the
            // read-lock check above and taking the write lock here -- don't
            // clobber it if so.
            run_info_locked
                .unit_table
                .entry(id.clone())
                .or_insert(unit);
            id
        }
    };

    // Flip found + status, and collect who needs to be (re)triggered, all while
    // holding only a read lock (mutating the unit's own state.found happens
    // through its RwLock, same as every other unit kind).
    let (required_by, wanted_by) = {
        let run_info_locked = run_info.read().unwrap();
        let unit = match run_info_locked.unit_table.get(&unit_id) {
            Some(unit) => unit,
            None => return,
        };
        if let Specific::Device(specific) = &unit.specific {
            let mut state = specific.state.write().unwrap();
            state.found = now_present;
        }
        {
            let mut status = unit.common.status.write().unwrap();
            *status = if now_present {
                UnitStatus::Started(StatusStarted::Running)
            } else {
                UnitStatus::Stopped(StatusStopped::StoppedFinal, vec![])
            };
        }
        (
            unit.common.dependencies.required_by.clone(),
            unit.common.dependencies.wanted_by.clone(),
        )
    };

    // Best-effort: try to (re)activate everything that depends on this device now
    // that it showed up, or stop everything that required it now that it's gone.
    // Errors are expected and common here (e.g. a dependent whose *other*
    // dependencies aren't ready yet just stays NeverStarted/Stopped until they
    // are) so they're only traced, never logged as a hard error.
    let run_info_locked = run_info.read().unwrap();
    for dependent_id in required_by.iter().chain(wanted_by.iter()) {
        if now_present {
            if let Err(e) = activate_unit(
                dependent_id.clone(),
                &*run_info_locked,
                ActivationSource::Regular,
            ) {
                ui::log(format!(
                    "Device {} appeared, but dependent {} did not (yet) activate: {}",
                    unit_id.name,
                    dependent_id,
                    e
                ));
            }
        } else {
            if let Err(e) = deactivate_unit(dependent_id, &*run_info_locked) {
                ui::log(format!(
                    "Device {} disappeared, but dependent {} did not (yet) deactivate: {}",
                    unit_id.name,
                    dependent_id,
                    e
                ));
            }
        }
    }
}
