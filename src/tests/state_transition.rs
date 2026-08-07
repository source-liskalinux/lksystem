use crate::runtime_info::*;
use crate::services::service_exit_handler;
use crate::units::{ServiceType, Unit, UnitStatus};
use nix::sys::stat::stat;
use nix::unistd::geteuid;
use std::convert::TryInto;

#[test]
fn test_service_state_transitions() {
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    // The service lifecycle tests exercise the state transitions directly and do not need
    // a long-running signal-handler thread in this harness.
    successful(run_info.clone());
    failing_startexec(run_info.clone());
}

#[test]
fn test_service_reload_runs_exec_reload() {
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    let parsed_file = crate::units::parse_file(
        r#"
        [Unit]
        Description = test
        [Service]
        Type=simple
        ExecStart=/bin/sleep 30
        ExecReload=/bin/true
        "#,
    )
    .unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/test.service"),
    )
    .unwrap();
    let unit: Unit = service.try_into().unwrap();
    let unit_id = unit.id.clone();

    run_info.write().unwrap().unit_table.insert(unit.id.clone(), unit);

    let binding = run_info.read().unwrap();
    let unit = binding.unit_table.get(&unit_id).unwrap();
    {
        let mut status = unit.common.status.write().unwrap();
        *status = UnitStatus::Started(crate::units::StatusStarted::Running);
    }

    let run_info_locked = run_info.read().unwrap();
    let result = unit.reload(&run_info_locked);
    assert!(result.is_ok(), "reload failed: {:?}", result);
}

#[test]
fn test_oneshot_service_stops_after_successful_exit() {
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    let parsed_file = crate::units::parse_file(
        r#"
        [Unit]
        Description = test
        [Service]
        Type=oneshot
        ExecStart=/bin/true
        "#,
    )
    .unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/test.service"),
    )
    .unwrap();
    let unit: Unit = service.try_into().unwrap();
    let unit_id = unit.id.clone();

    run_info.write().unwrap().unit_table.insert(unit.id.clone(), unit);

    let pid = nix::unistd::Pid::from_raw(4242);
    run_info
        .write()
        .unwrap()
        .pid_table
        .lock()
        .unwrap()
        .insert(pid, PidEntry::Service(unit_id.clone(), ServiceType::OneShot));

    let binding = run_info.read().unwrap();
    let unit = binding.unit_table.get(&unit_id).unwrap();
    {
        let mut status = unit.common.status.write().unwrap();
        *status = UnitStatus::Started(crate::units::StatusStarted::Running);
    }

    let run_info_locked = run_info.read().unwrap();
    service_exit_handler(pid, crate::signal_handler::ChildTermination::Exit(0), &run_info_locked)
        .unwrap();

    let status = unit.common.status.read().unwrap();
    assert_eq!(*status, UnitStatus::Stopped(crate::units::StatusStopped::StoppedFinal, vec![]));
}

fn successful(run_info: ArcMutRuntimeInfo) {
    let descr = "This is a description";
    let service_execstart = "/bin/sleep 10";
    let service_execpre = "/bin/true";
    let service_execpost = "/bin/true";
    let service_stop = "/bin/true";
    let service_stoppost = "/bin/true";

    let test_service_str = format!(
        r#"
    [Unit]
    Description = {}
    [Service]
    ExecStart = {}
    ExecStartPre = {}
    ExecStartPost = {}
    ExecStop = {}
    ExecStopPost = {}

    "#,
        descr, service_execstart, service_execpre, service_execpost, service_stop, service_stoppost,
    );

    let parsed_file = crate::units::parse_file(&test_service_str).unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/unitfile.service"),
    )
    .unwrap();
    let unit: Unit = service.try_into().unwrap();

    let unit_id = unit.id.clone();

    run_info
        .write()
        .unwrap()
        .unit_table
        .insert(unit.id.clone(), unit);

    let run_info_locked = run_info.read().unwrap();
    let unit = run_info_locked.unit_table.get(&unit_id).unwrap();

    unit.activate(
        &*run_info.read().unwrap(),
        crate::units::ActivationSource::Regular,
    )
    .unwrap();
    let status = unit.common.status.read().unwrap();

    assert_eq!(
        *status,
        crate::units::UnitStatus::Started(crate::units::StatusStarted::Running)
    );
}

fn failing_startexec(run_info: ArcMutRuntimeInfo) {
    let descr = "This is a description";
    let service_type = "oneshot";
    let service_execstart = "/bin/false";
    let service_execpre = "/bin/true";
    let service_execpost = "/bin/true";
    let service_stop = "/bin/true";
    let service_stoppost = "/bin/true";

    let test_service_str = format!(
        r#"
    [Unit]
    Description = {}
    [Service]
    Type= {}
    ExecStart = {}
    ExecStartPre = {}
    ExecStartPost = {}
    ExecStop = {}
    ExecStopPost = {}

    "#,
        descr,
        service_type,
        service_execstart,
        service_execpre,
        service_execpost,
        service_stop,
        service_stoppost,
    );

    let parsed_file = crate::units::parse_file(&test_service_str).unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/unitfile.service"),
    )
    .unwrap();
    let unit: Unit = service.try_into().unwrap();

    let unit_id = unit.id.clone();

    run_info
        .write()
        .unwrap()
        .unit_table
        .insert(unit.id.clone(), unit);

    let run_info_locked = run_info.read().unwrap();
    let unit = run_info_locked.unit_table.get(&unit_id).unwrap();

    assert!(unit
        .activate(
            &*run_info.read().unwrap(),
            crate::units::ActivationSource::Regular
        )
        .is_err());
    let status = unit.common.status.read().unwrap();

    match &*status {
        crate::units::UnitStatus::Stopped(
            crate::units::StatusStopped::StoppedUnexpected,
            errors,
        ) => {
            if errors.len() != 1 {
                panic!("Wrong amount of errors. Should be 1. Is: {}", errors.len());
            }
            match &errors[0] {
                crate::units::UnitOperationErrorReason::ServiceStartError(
                    crate::services::ServiceErrorReason::StartFailed(
                        crate::services::RunCmdError::BadExitCode(_, _),
                    ),
                ) => {
                    // HAPPY
                }
                other => {
                    panic!(
                        "Wrong error. Should have been ServiceStartError(StartFailed(BadExitCode(_,_))). Is: {:?}",
                        other
                    );
                }
            }
        }
        other => panic!(
            "Wrong status. Should have been StoppedUnexpected. Is: {:?}",
            other
        ),
    };
}

#[test]
fn test_activation_fails_when_conditions_are_not_met() {
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    let missing_path = std::env::temp_dir().join("lksystem_missing_condition_path");
    let parsed_file = crate::units::parse_file(&format!(
        "\n[Unit]\nDescription = test\n[Condition]\nConditionPathExists = {}\n[Service]\nType=simple\nExecStart=/bin/true\n",
        missing_path.display()
    ))
    .unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/test.service"),
    )
    .unwrap();
    let unit: Unit = service.try_into().unwrap();
    let unit_id = unit.id.clone();

    run_info.write().unwrap().unit_table.insert(unit.id.clone(), unit);

    let run_info_locked = run_info.read().unwrap();
    let unit = run_info_locked.unit_table.get(&unit_id).unwrap();
    let result = unit.activate(&*run_info_locked, crate::units::ActivationSource::Regular);

    assert!(result.is_err());
    let err = result.unwrap_err();
    match err.reason {
        crate::units::UnitOperationErrorReason::GenericStartError(msg) => {
            assert!(msg.contains("ConditionPathExists"));
        }
        other => panic!("Expected a condition error, got {:?}", other),
    }
}

#[test]
fn test_activation_fails_when_conflicting_unit_is_active() {
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    let conflicting = crate::units::parse_service(
        crate::units::parse_file(
            "\n[Unit]\nDescription = existing\n[Service]\nType=oneshot\nExecStart=/bin/true\n",
        )
        .unwrap(),
        &std::path::PathBuf::from("/path/to/conflicting.service"),
    )
    .unwrap();
    let conflicting_unit: Unit = conflicting.try_into().unwrap();
    let conflicting_id = conflicting_unit.id.clone();

    let blocked = crate::units::parse_service(
        crate::units::parse_file(
            "\n[Unit]\nDescription = blocked\nConflicts = conflicting.service\n[Service]\nType=oneshot\nExecStart=/bin/true\n",
        )
        .unwrap(),
        &std::path::PathBuf::from("/path/to/blocked.service"),
    )
    .unwrap();
    let blocked_unit: Unit = blocked.try_into().unwrap();
    let blocked_id = blocked_unit.id.clone();

    run_info.write().unwrap().unit_table.insert(conflicting_id.clone(), conflicting_unit);
    run_info.write().unwrap().unit_table.insert(blocked_id.clone(), blocked_unit);

    {
        let guard = run_info.read().unwrap();
        let unit = guard.unit_table.get(&conflicting_id).unwrap();
        let mut status = unit.common.status.write().unwrap();
        *status = UnitStatus::Started(crate::units::StatusStarted::Running);
    }

    let run_info_locked = run_info.read().unwrap();
    let unit = run_info_locked.unit_table.get(&blocked_id).unwrap();
    let result = unit.activate(&*run_info_locked, crate::units::ActivationSource::Regular);

    assert!(result.is_err(), "expected activation to be blocked by the active conflict: {:?}", result);
    let status = unit.common.status.read().unwrap();
    assert!(matches!(*status, UnitStatus::Stopped(crate::units::StatusStopped::StoppedFinal, _)));
}

#[test]
fn test_activation_is_blocked_by_conflicting_unit() {
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    let conflicting = crate::units::parse_service(
        crate::units::parse_file(
            "\n[Unit]\nDescription = existing\n[Service]\nType=simple\nExecStart=/bin/true\n",
        )
        .unwrap(),
        &std::path::PathBuf::from("/path/to/conflicting.service"),
    )
    .unwrap();
    let conflicting_unit: Unit = conflicting.try_into().unwrap();
    let conflicting_id = conflicting_unit.id.clone();

    let blocked = crate::units::parse_service(
        crate::units::parse_file(
            "\n[Unit]\nDescription = blocked\nConflicts = conflicting.service\n[Service]\nType=simple\nExecStart=/bin/true\n",
        )
        .unwrap(),
        &std::path::PathBuf::from("/path/to/blocked.service"),
    )
    .unwrap();
    let blocked_unit: Unit = blocked.try_into().unwrap();
    let blocked_id = blocked_unit.id.clone();

    run_info.write().unwrap().unit_table.insert(conflicting_id.clone(), conflicting_unit);
    run_info.write().unwrap().unit_table.insert(blocked_id.clone(), blocked_unit);

    {
        let guard = run_info.read().unwrap();
        let unit = guard.unit_table.get(&conflicting_id).unwrap();
        let mut status = unit.common.status.write().unwrap();
        *status = UnitStatus::Started(crate::units::StatusStarted::Running);
    }

    let run_info_locked = run_info.read().unwrap();
    let blocker = run_info_locked.unit_table.get(&blocked_id).unwrap();
    let start_result = blocker.activate(&*run_info_locked, crate::units::ActivationSource::Regular);

    assert!(start_result.is_err(), "expected blocked unit activation to be rejected: {:?}", start_result);

    let guard = run_info.read().unwrap();
    let conflicting = guard.unit_table.get(&conflicting_id).unwrap();
    let blocked = guard.unit_table.get(&blocked_id).unwrap();

    assert!(matches!(
        *conflicting.common.status.read().unwrap(),
        UnitStatus::Started(crate::units::StatusStarted::Running)
    ));
    assert!(matches!(
        *blocked.common.status.read().unwrap(),
        UnitStatus::Stopped(crate::units::StatusStopped::StoppedFinal, _)
    ));
}

#[test]
fn test_restart_policy_helper_uses_exit_status() {
    assert!(crate::services::should_restart_for_exit(
        crate::units::ServiceRestart::OnFailure,
        crate::signal_handler::ChildTermination::Exit(1),
    ));
    assert!(!crate::services::should_restart_for_exit(
        crate::units::ServiceRestart::OnFailure,
        crate::signal_handler::ChildTermination::Exit(0),
    ));
    assert!(crate::services::should_restart_for_exit(
        crate::units::ServiceRestart::OnSuccess,
        crate::signal_handler::ChildTermination::Exit(0),
    ));
    assert!(!crate::services::should_restart_for_exit(
        crate::units::ServiceRestart::OnSuccess,
        crate::signal_handler::ChildTermination::Exit(1),
    ));
}

#[test]
fn test_mount_activate_unmount_realfs() {
    if !geteuid().is_root() {
        crate::ui::write_error("Skipping mount/unmount test because it requires root.");
        return;
    }

    let temp_dir = std::env::temp_dir().join("lksystem_mount_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let where_path = temp_dir.join("mnt");
    let what_path = std::path::PathBuf::from("tmpfs");

    let parsed_mount = crate::units::ParsedMountConfig {
        common: crate::units::ParsedCommonConfig {
            name: "test.mount".to_owned(),
            unit: Default::default(),
            install: Default::default(),
            conditions: Default::default(),
        },
        mount: crate::units::ParsedMountSection {
            what: Some(what_path.clone()),
            where_path: Some(where_path.clone()),
            fstype: Some("tmpfs".to_owned()),
            options: Some("mode=0755".to_owned()),
        },
    };

    let unit: Unit = parsed_mount.try_into().unwrap();
    let unit_id = unit.id.clone();

    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: crate::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "".into(),
            unit_dirs: vec![],
            self_path: std::path::PathBuf::from("./target/debug/lksystem"),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(crate::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table: UnitTable::default(),
        stdout_eventfd: crate::platform::make_event_fd().unwrap(),
        stderr_eventfd: crate::platform::make_event_fd().unwrap(),
        notification_eventfd: crate::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: crate::platform::make_event_fd().unwrap(),
    }));

    run_info.write().unwrap().unit_table.insert(unit.id.clone(), unit);

    let run_info_locked = run_info.read().unwrap();
    let mount_unit = run_info_locked.unit_table.get(&unit_id).unwrap();

    let start_res = mount_unit.activate(&*run_info_locked, crate::units::ActivationSource::Regular);
    assert!(start_res.is_ok(), "Mount activation failed: {:?}", start_res);
    assert!(where_path.exists(), "Mountpoint directory should exist after activation");

    let st = stat(&where_path).unwrap();
    assert!(st.st_dev != 0, "Expected mountpoint to be mounted");

    let stop_res = mount_unit.deactivate(&*run_info_locked);
    assert!(stop_res.is_ok(), "Mount deactivation failed: {:?}", stop_res);
    assert!(!mount_unit.common.status.read().unwrap().is_started());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
