//! Path unit activation support. Watches `PathExists=` paths and triggers the
//! referenced `Unit=` when the path appears.

use crate::runtime_info::ArcMutRuntimeInfo;
use crate::units::*;
use crate::ui;
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(target_os = "linux")]
pub fn start_path_events_thread(run_info: ArcMutRuntimeInfo) {
    if let Err(e) = spawn_path_listener(run_info.clone()) {
        ui::warning(format!(
            "Could not start path event listener, .path units will not be triggered: {}",
            e
        ));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn start_path_events_thread(_run_info: ArcMutRuntimeInfo) {
    ui::warning(format!(".path unit activation is only supported on Linux"));
}

#[cfg(target_os = "linux")]
fn spawn_path_listener(run_info: ArcMutRuntimeInfo) -> std::io::Result<()> {
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let mut watched_dirs: HashMap<PathBuf, Vec<UnitId>> = HashMap::new();
    {
        let run_info_locked = run_info.read().unwrap();
        for unit in run_info_locked.unit_table.values() {
            if let Specific::Path(specific) = &unit.specific {
                let path_exists = &specific.conf.path_exists;
                let parent = path_exists.parent().unwrap_or_else(|| Path::new("/")).to_path_buf();
                let watch_dir = find_existing_parent(&parent);
                watched_dirs.entry(watch_dir).or_default().push(unit.id.clone());
            }
        }
    }

    if watched_dirs.is_empty() {
        return Ok(());
    }

    let mut watch_to_dir = HashMap::new();
    for watch_dir in watched_dirs.keys() {
        let c_path = std::ffi::CString::new(watch_dir.as_os_str().as_bytes()).unwrap();
        let wd = unsafe {
            libc::inotify_add_watch(
                fd,
                c_path.as_ptr(),
                libc::IN_CREATE
                    | libc::IN_DELETE
                    | libc::IN_MOVED_FROM
                    | libc::IN_MOVED_TO
                    | libc::IN_DELETE_SELF
                    | libc::IN_MOVE_SELF,
            )
        };
        if wd < 0 {
            ui::warning(format!("Could not watch directory {:?}: {}", watch_dir, std::io::Error::last_os_error()));
        } else {
            watch_to_dir.insert(wd, watch_dir.clone());
        }
    }

    if watch_to_dir.is_empty() {
        ui::warning(format!("No path watch descriptors were successfully created"));
        return Ok(());
    }

    check_path_units(&run_info);

    std::thread::Builder::new()
        .name("path-events".to_owned())
        .spawn(move || {
            if let Err(e) = path_event_loop(fd, watch_to_dir, run_info) {
                ui::error(format!("path event loop stopped: {}", e));
            }
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn find_existing_parent(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    while !current.exists() {
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    current
}

#[cfg(target_os = "linux")]
fn path_event_loop(
    fd: libc::c_int,
    _watch_to_dir: HashMap<libc::c_int, PathBuf>,
    run_info: ArcMutRuntimeInfo,
) -> std::io::Result<()> {
    let mut buffer = vec![0u8; 4096];

    loop {
        let len = unsafe {
            libc::read(
                fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
            )
        };
        if len < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            return Err(err);
        }
        if len == 0 {
            continue;
        }

        let mut offset = 0;
        while offset < len as usize {
            let event_ptr = unsafe { buffer.as_ptr().add(offset) as *const libc::inotify_event };
            let event = unsafe { std::ptr::read_unaligned(event_ptr) };
            let event_size = std::mem::size_of::<libc::inotify_event>();
            let name_len = event.len as usize;
            let _name = if name_len > 0 {
                let name_start = offset + event_size;
                let name_end = name_start + name_len;
                let raw_name = &buffer[name_start..name_end];
                let nul_end = raw_name.iter().position(|b| *b == 0).unwrap_or(raw_name.len());
                std::str::from_utf8(&raw_name[..nul_end]).unwrap_or_default().to_owned()
            } else {
                String::new()
            };

            ui::log(format!("path event: wd={} mask={:#x} name={}", event.wd, event.mask, _name));
            check_path_units(&run_info);

            offset += event_size + name_len;
        }
    }
}

fn check_path_units(run_info: &ArcMutRuntimeInfo) {
    let path_units: Vec<(UnitId, PathBuf, bool)> = {
        let run_info_locked = run_info.read().unwrap();
        run_info_locked
            .unit_table
            .values()
            .filter_map(|unit| {
                if let Specific::Path(specific) = &unit.specific {
                    let found = specific.state.read().unwrap().found;
                    Some((unit.id.clone(), specific.conf.path_exists.clone(), found))
                } else {
                    None
                }
            })
            .collect()
    };

    for (unit_id, path_exists, previously_found) in path_units {
        let exists = path_exists.exists();
        if exists && !previously_found {
            if let Some(unit) = run_info.read().unwrap().unit_table.get(&unit_id) {
                if let Specific::Path(specific) = &unit.specific {
                    let mut state = specific.state.write().unwrap();
                    state.found = true;
                }
            }

            let run_info_locked = run_info.read().unwrap();
            if let Err(e) = activate_unit(unit_id.clone(), &*run_info_locked, ActivationSource::Regular) {
                ui::log(format!(
                    "Path unit {} matched but did not activate: {}",
                    unit_id.name,
                    e
                ));
            }
        } else if !exists && previously_found {
            if let Some(unit) = run_info.read().unwrap().unit_table.get(&unit_id) {
                if let Specific::Path(specific) = &unit.specific {
                    let mut state = specific.state.write().unwrap();
                    state.found = false;
                }
            }
        }
    }
}
