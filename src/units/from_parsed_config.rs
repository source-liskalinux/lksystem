use crate::platform::device_unit_name_from_devname;
use crate::services::*;
use crate::sockets::*;
use crate::units::*;

#[cfg(target_os = "linux")]
use crate::ui;

use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::RwLock;

#[cfg(target_os = "linux")]
fn make_cgroup_path(
    srvc_name: &str,
    uid: nix::unistd::Uid,
    slice: Option<&String>,
) -> Result<PathBuf, String> {
    let lksystem_cgroup =
        crate::platform::cgroups::get_own_freezer(&PathBuf::from("/sys/fs/cgroup"))
            .map_err(|e| format!("Couldnt get own cgroup: {}", e))?;

    let mut service_cgroup = if uid.as_raw() != 0 {
        lksystem_cgroup
            .join("user.slice")
            .join(format!("user-{}.slice", uid.as_raw()))
    } else {
        lksystem_cgroup.join("system.slice")
    };

    if let Some(slice_name) = slice {
        let slice_name = if slice_name.ends_with(".slice") {
            slice_name.to_owned()
        } else {
            format!("{}.slice", slice_name)
        };
        service_cgroup = service_cgroup.join(slice_name);
    }

    let service_name = if srvc_name.ends_with(".service") {
        srvc_name.to_owned()
    } else {
        format!("{}.service", srvc_name)
    };
    service_cgroup = service_cgroup.join(service_name);
    ui::log(format!(
        "Service {} (uid={}) will be moved into cgroup: {:?}",
        srvc_name,
        uid.as_raw(),
        service_cgroup
    ));
    Ok(service_cgroup)
}

#[cfg(not(target_os = "linux"))]
fn make_cgroup_path(
    _srvc_name: &str,
    _uid: nix::unistd::Uid,
    _slice: Option<&String>,
) -> Result<PathBuf, String> {
    // doesnt matter, wont be used anyways
    Ok(PathBuf::from("/ree"))
}

pub fn unit_from_parsed_service(conf: ParsedServiceConfig) -> Result<Unit, String> {
    // TODO make the cgroup path dynamic so multiple lksystem instances can exist
    // Build exec_config first so we know the target uid for cgroup placement
    let exec_conf: crate::units::ExecConfig = conf.srvc.exec_section.try_into()?;

    let platform_specific = PlatformSpecificServiceFields {
        #[cfg(target_os = "linux")]
        cgroup_path: make_cgroup_path(&conf.common.name, exec_conf.user, conf.srvc.slice.as_ref())?,
    };

    let mut sockets: Vec<UnitId> = Vec::new();
    for sock in conf.srvc.sockets {
        sockets.push(sock.as_str().try_into()?);
    }

    let mut common = make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?;
    common.unit.refs_by_name.extend(sockets.iter().cloned());

    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Service,
            name: conf.common.name,
        },
        common,
        specific: Specific::Service(ServiceSpecific {
                conf: ServiceConfig {
                exec_config: exec_conf,
                sockets: sockets,
                accept: conf.srvc.accept,
                dbus_name: conf.srvc.dbus_name,
                restart: conf.srvc.restart,
                notifyaccess: conf.srvc.notifyaccess,
                exec: conf.srvc.exec,
                startpre: conf.srvc.startpre,
                startpost: conf.srvc.startpost,
                stop: conf.srvc.stop,
                stoppost: conf.srvc.stoppost,
                srcv_type: conf.srvc.srcv_type,
                starttimeout: conf.srvc.starttimeout,
                stoptimeout: conf.srvc.stoptimeout,
                generaltimeout: conf.srvc.generaltimeout,
                reload: conf.srvc.reload,
                remain_after_exit: conf.srvc.remain_after_exit,
                restart_sec: conf.srvc.restart_sec,
                slice: conf.srvc.slice,
                cpu_quota: conf.srvc.cpu_quota,
                cpu_weight: conf.srvc.cpu_weight,
                memory_max: conf.srvc.memory_max,
                tasks_max: conf.srvc.tasks_max,
                io_weight: conf.srvc.io_weight,
                platform_specific,
            },
            state: RwLock::new(ServiceState {
                common: CommonState::default(),
                srvc: Service {
                    pid: None,
                    status_msgs: Vec::new(),
                    process_group: None,
                    signaled_ready: false,
                    notifications: None,
                    notifications_path: None,
                    stdout: None,
                    stderr: None,
                    notifications_buffer: String::new(),
                    stdout_buffer: Vec::new(),
                    stderr_buffer: Vec::new(),
                },
            }),
        }),
    })
}

pub fn unit_from_parsed_socket(conf: ParsedSocketConfig) -> Result<Unit, String> {
    let mut services: Vec<UnitId> = Vec::new();
    for srvc in conf.sock.services {
        services.push(srvc.as_str().try_into()?);
    }

    let mut common = make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?;
    common.unit.refs_by_name.extend(services.iter().cloned());

    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Socket,
            name: conf.common.name,
        },
        common,
        specific: Specific::Socket(SocketSpecific {
            conf: SocketConfig {
                exec_config: conf.sock.exec_section.try_into()?,
                filedesc_name: conf.sock.filedesc_name.unwrap_or("unknown".to_owned()),
                services: services,
                sockets: conf.sock.sockets.into_iter().map(Into::into).collect(),
            },
            state: RwLock::new(SocketState {
                common: CommonState::default(),
                sock: Socket { activated: false },
            }),
        }),
    })
}
pub fn unit_from_parsed_target(conf: ParsedTargetConfig) -> Result<Unit, String> {
    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Target,
            name: conf.common.name,
        },
        common: make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?,
        specific: Specific::Target(TargetSpecific {
            state: RwLock::new(TargetState {
                common: CommonState::default(),
            }),
        }),
    })
}

pub fn unit_from_parsed_device(conf: ParsedDeviceConfig) -> Result<Unit, String> {
    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Device,
            name: conf.common.name,
        },
        common: make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?,
        specific: Specific::Device(DeviceSpecific {
            state: RwLock::new(DeviceState {
                common: CommonState::default(),
                // A statically defined .device file describes a device we have not
                // necessarily seen a uevent for yet (e.g. it's loaded before the
                // coldplug pass runs). crate::device_events flips this to true once
                // a matching uevent (or coldplug replay) is observed.
                found: false,
            }),
        }),
    })
}

pub fn unit_from_parsed_mount(conf: ParsedMountConfig) -> Result<Unit, String> {
    let what = conf
        .mount
        .what
        .clone()
        .expect("What= is always filled in by parse_mount");
    let where_path = conf
        .mount
        .where_path
        .clone()
        .expect("Where= is always filled in by parse_mount");

    let mut common = make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?;
    if let Ok(device_name_path) = what.strip_prefix("/dev/") {
        if let Some(device_name) = device_name_path.to_str() {
            if !device_name.is_empty() {
                let device_unit_name = device_unit_name_from_devname(device_name);
                let device_unit_id = UnitId {
                    kind: UnitIdKind::Device,
                    name: device_unit_name.clone(),
                };
                common.dependencies.requires.push(device_unit_id.clone());
                common.dependencies.after.push(device_unit_id.clone());
                common.unit.refs_by_name.push(device_unit_id);
            }
        }
    }

    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Mount,
            name: conf.common.name,
        },
        common,
        specific: Specific::Mount(MountSpecific {
            conf: MountConfig {
                what,
                where_path,
                fstype: conf.mount.fstype,
                options: conf.mount.options,
            },
            state: RwLock::new(MountState {
                common: CommonState::default(),
                mounted: false,
            }),
        }),
    })
}

pub fn unit_from_parsed_path(conf: ParsedPathConfig) -> Result<Unit, String> {
    let target_unit: UnitId = conf
        .path
        .unit
        .as_deref()
        .expect("Unit= is always filled in by parse_path")
        .try_into()?;

    let mut common = make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?;
    // So `insert_new_units`/`check_all_names_exist` catch a Unit= that points
    // at a unit which doesn't (and won't) exist, same as it does for a
    // service's Sockets=.
    common.unit.refs_by_name.push(target_unit.clone());

    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Path,
            name: conf.common.name,
        },
        common,
        specific: Specific::Path(PathSpecific {
                conf: PathConfig {
                path_exists: conf.path.path_exists.expect("PathExists set by parser"),
                unit: target_unit,
            },
            state: RwLock::new(PathState {
                common: CommonState::default(),
                found: false,
            }),
        }),
    })
}

pub fn unit_from_parsed_timer(conf: ParsedTimerConfig) -> Result<Unit, String> {
    let target_unit: UnitId = conf
        .timer
        .unit
        .as_deref()
        .expect("Unit= is always filled in by parse_timer")
        .try_into()?;

    let mut common = make_common_from_parsed(conf.common.unit, conf.common.install, conf.common.conditions)?;
    common.unit.refs_by_name.push(target_unit.clone());

    Ok(Unit {
        id: UnitId {
            kind: UnitIdKind::Timer,
            name: conf.common.name,
        },
        common,
        specific: Specific::Timer(TimerSpecific {
            conf: TimerConfig {
                on_boot_sec: conf.timer.on_boot_sec,
                on_active_sec: conf.timer.on_active_sec,
                on_calendar: conf.timer.on_calendar.clone(),
                on_unit_active_sec: conf.timer.on_unit_active_sec,
                unit: target_unit,
            },
            state: RwLock::new(TimerState {
                common: CommonState::default(),
                last_trigger: None,
            }),
        }),
    })
}

impl From<ParsedSingleSocketConfig> for SingleSocketConfig {
    fn from(parsed: ParsedSingleSocketConfig) -> SingleSocketConfig {
        SingleSocketConfig {
            kind: parsed.kind,
            specialized: parsed.specialized,
        }
    }
}

impl std::convert::TryFrom<ParsedExecSection> for ExecConfig {
    type Error = String;
    fn try_from(parsed: ParsedExecSection) -> Result<ExecConfig, String> {
        let uid = if let Some(user) = &parsed.user {
            if let Ok(uid) = user.parse::<u32>() {
                Some(nix::unistd::Uid::from_raw(uid))
            } else {
                if let Ok(pwentry) = crate::platform::pwnam::getpwnam_r(&user)
                    .map_err(|e| ParsingErrorReason::Generic(e))
                {
                    Some(pwentry.uid)
                } else {
                    return Err(format!("Couldnt get uid for username: {}", user));
                }
            }
        } else {
            None
        };
        let uid = uid.unwrap_or(nix::unistd::getuid());

        let gid = if let Some(group) = &parsed.group {
            if let Ok(gid) = group.parse::<u32>() {
                Some(nix::unistd::Gid::from_raw(gid))
            } else {
                if let Ok(groupentry) = crate::platform::grnam::getgrnam_r(&group)
                    .map_err(|e| ParsingErrorReason::Generic(e))
                {
                    Some(groupentry.gid)
                } else {
                    return Err(format!("Couldnt get gid for groupname: {}", group));
                }
            }
        } else {
            None
        };
        let gid = gid.unwrap_or(nix::unistd::getgid());

        let mut supp_gids = Vec::new();
        for group in &parsed.supplementary_groups {
            let gid = if let Ok(gid) = group.parse::<u32>() {
                nix::unistd::Gid::from_raw(gid)
            } else {
                if let Ok(groupentry) = crate::platform::grnam::getgrnam_r(&group)
                    .map_err(|e| ParsingErrorReason::Generic(e))
                {
                    groupentry.gid
                } else {
                    return Err(format!("Couldnt get gid for groupname: {}", group));
                }
            };
            supp_gids.push(gid);
        }
        Ok(ExecConfig {
            user: uid,
            group: gid,
            working_directory: parsed.working_directory,
            supplementary_groups: supp_gids,
            stderr_path: parsed.stderr_path,
            stdout_path: parsed.stdout_path,
            environment: parsed.environment,
            environment_files: parsed.environment_files,
        })
    }
}

fn make_common_from_parsed(
    unit: ParsedUnitSection,
    install: ParsedInstallSection,
    conditions: ParsedConditions,
) -> Result<Common, String> {
    let mut wants = Vec::new();
    for name in unit.wants {
        wants.push(name.as_str().try_into()?);
    }
    let mut requires = Vec::new();
    for name in unit.requires {
        requires.push(name.as_str().try_into()?);
    }
    let mut wanted_by = Vec::new();
    for name in install.wanted_by {
        wanted_by.push(name.as_str().try_into()?);
    }
    let mut required_by = Vec::new();
    for name in install.required_by {
        required_by.push(name.as_str().try_into()?);
    }
    let mut after = Vec::new();
    for name in unit.after {
        after.push(name.as_str().try_into()?);
    }
    let mut before = Vec::new();
    for name in unit.before {
        before.push(name.as_str().try_into()?);
    }

    let mut refs_by_name = Vec::new();
    refs_by_name.extend(wants.iter().cloned());
    refs_by_name.extend(wanted_by.iter().cloned());
    refs_by_name.extend(requires.iter().cloned());
    refs_by_name.extend(required_by.iter().cloned());
    refs_by_name.extend(before.iter().cloned());
    refs_by_name.extend(after.iter().cloned());

    Ok(Common {
        status: RwLock::new(UnitStatus::NeverStarted),
        unit: UnitConfig {
            description: unit.description,
            refs_by_name,
            conflicts: unit.conflicts,
        },
        conditions,
        dependencies: Dependencies {
            wants,
            wanted_by,
            requires,
            required_by,
            after,
            before,
        },
    })
}

impl std::convert::TryInto<UnitId> for &str {
    type Error = String;
    fn try_into(self) -> Result<UnitId, String> {
        if self.ends_with(".target") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Target,
            })
        } else if self.ends_with(".service") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Service,
            })
        } else if self.ends_with(".socket") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Socket,
            })
        } else if self.ends_with(".device") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Device,
            })
        } else if self.ends_with(".timer") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Timer,
            })
        } else if self.ends_with(".mount") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Mount,
            })
        } else if self.ends_with(".path") {
            Ok(UnitId {
                name: self.to_owned(),
                kind: UnitIdKind::Path,
            })
        } else {
            Err(format!(
                "{} is not a valid unit name. The suffix is not supported.",
                self
            ))
        }
    }
}

impl std::convert::TryFrom<ParsedServiceConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedServiceConfig) -> Result<Unit, String> {
        unit_from_parsed_service(conf)
    }
}
impl std::convert::TryFrom<ParsedSocketConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedSocketConfig) -> Result<Unit, String> {
        unit_from_parsed_socket(conf)
    }
}
impl std::convert::TryFrom<ParsedTargetConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedTargetConfig) -> Result<Unit, String> {
        unit_from_parsed_target(conf)
    }
}
impl std::convert::TryFrom<ParsedDeviceConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedDeviceConfig) -> Result<Unit, String> {
        unit_from_parsed_device(conf)
    }
}
impl std::convert::TryFrom<ParsedTimerConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedTimerConfig) -> Result<Unit, String> {
        unit_from_parsed_timer(conf)
    }
}
impl std::convert::TryFrom<ParsedMountConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedMountConfig) -> Result<Unit, String> {
        unit_from_parsed_mount(conf)
    }
}
impl std::convert::TryFrom<ParsedPathConfig> for Unit {
    type Error = String;
    fn try_from(conf: ParsedPathConfig) -> Result<Unit, String> {
        unit_from_parsed_path(conf)
    }
}
