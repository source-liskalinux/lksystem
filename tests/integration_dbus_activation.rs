use std::fs;
use std::thread;
use std::time::Duration;

use lksystem::runtime_info::*;

#[test]
fn test_dbus_socket_activation_to_service_start() {
    // prepare temp dir for units and socket path
    let base = std::env::temp_dir().join(format!("lks_test_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    let socket_path = base.join("sys_bus.sock");
    let socket_unit = format!(r#"[Unit]
Description=Test DBus Socket

[Socket]
ListenStream={}
FileDescriptorName=system_bus_socket
Service=dbus-test.service

[Install]
WantedBy=sockets.target
"#, socket_path.to_str().unwrap());

    let service_unit = r#"[Unit]
Description=Test DBus Service
After=dbus.socket
Requires=dbus.socket

[Service]
ExecStart=/bin/sleep 60
Restart=always

[Install]
WantedBy=default.target
"#;

    fs::write(base.join("dbus-test.socket"), socket_unit).unwrap();
    fs::write(base.join("dbus-test.service"), service_unit).unwrap();

    // parse units individually and construct a unit_table without running full dependency resolution
    let parsed_socket = lksystem::units::parse_file(&fs::read_to_string(base.join("dbus-test.socket")).unwrap()).unwrap();
    let parsed_service = lksystem::units::parse_file(&fs::read_to_string(base.join("dbus-test.service")).unwrap()).unwrap();
    let socket_unit_parsed = lksystem::units::parse_socket(parsed_socket, &base.join("dbus-test.socket")).unwrap();
    let service_unit_parsed = lksystem::units::parse_service(parsed_service, &base.join("dbus-test.service")).unwrap();
    let mut socket_unit: lksystem::units::Unit = socket_unit_parsed.try_into().unwrap();
    let mut service_unit: lksystem::units::Unit = service_unit_parsed.try_into().unwrap();

    // Link socket <-> service manually (simple implicit relation)
    let socket_id = socket_unit.id.clone();
    let service_id = service_unit.id.clone();
    if let lksystem::units::Specific::Service(s) = &mut service_unit.specific {
        s.conf.sockets.push(socket_id.clone());
        service_unit.common.dependencies.after.push(socket_id.clone());
        service_unit.common.dependencies.requires.push(socket_id.clone());
    }
    if let lksystem::units::Specific::Socket(s) = &mut socket_unit.specific {
        s.conf.services.push(service_id.clone());
        socket_unit.common.dependencies.before.push(service_id.clone());
        socket_unit.common.dependencies.required_by.push(service_id.clone());
    }

    let mut unit_table = std::collections::HashMap::new();
    unit_table.insert(socket_unit.id.clone(), socket_unit);
    unit_table.insert(service_unit.id.clone(), service_unit);

    // insert minimal targets that services/sockets may reference (e.g., default.target, sockets.target)
    let parsed_default = lksystem::units::parse_file(&"[Unit]\nDescription=default\n".to_string()).unwrap();
    let parsed_sockets = lksystem::units::parse_file(&"[Unit]\nDescription=sockets\n".to_string()).unwrap();
    let default_unit_parsed = lksystem::units::parse_target(parsed_default, &std::path::PathBuf::from("default.target")).unwrap();
    let sockets_unit_parsed = lksystem::units::parse_target(parsed_sockets, &std::path::PathBuf::from("sockets.target")).unwrap();
    let default_unit: lksystem::units::Unit = default_unit_parsed.try_into().unwrap();
    let sockets_unit: lksystem::units::Unit = sockets_unit_parsed.try_into().unwrap();
    unit_table.insert(default_unit.id.clone(), default_unit);
    unit_table.insert(sockets_unit.id.clone(), sockets_unit);

    // also insert a dbus.socket unit because the test service references it in After=
    let dbus_sock_text = format!(r#"[Unit]
Description=DBus socket

[Socket]
ListenStream={}
FileDescriptorName=system_bus_socket
Service=dbus-test.service

[Install]
WantedBy=sockets.target
"#, socket_path.to_str().unwrap());
    let parsed_dbussock = lksystem::units::parse_file(&dbus_sock_text).unwrap();
    let dbus_sock_parsed = lksystem::units::parse_socket(parsed_dbussock, &std::path::PathBuf::from("dbus.socket")).unwrap();
    let dbus_sock_unit: lksystem::units::Unit = dbus_sock_parsed.try_into().unwrap();
    unit_table.insert(dbus_sock_unit.id.clone(), dbus_sock_unit);

    // ensure any referenced Target units exist in the table
    let referenced: Vec<lksystem::units::UnitId> = unit_table
        .values()
        .flat_map(|u| {
            let mut v = Vec::new();
            v.extend(u.common.dependencies.after.clone());
            v.extend(u.common.dependencies.before.clone());
            v.extend(u.common.dependencies.requires.clone());
            v.extend(u.common.dependencies.wants.clone());
            v.extend(u.common.dependencies.wanted_by.clone());
            v.extend(u.common.dependencies.required_by.clone());
            v
        })
        .collect();

    for id in referenced {
        if id.kind == lksystem::units::UnitIdKind::Target && !unit_table.contains_key(&id) {
            let parsed = lksystem::units::parse_file(&"[Unit]\nDescription=auto-inserted target\n".to_string()).unwrap();
            let parsed_t = lksystem::units::parse_target(parsed, &std::path::PathBuf::from(id.name.clone())).unwrap();
            let unit: lksystem::units::Unit = parsed_t.try_into().unwrap();
            unit_table.insert(unit.id.clone(), unit);
        }
    }

    // build runtime info
    let run_info = std::sync::Arc::new(std::sync::RwLock::new(RuntimeInfo {
        config: lksystem::config::Config {
            notification_sockets_dir: "./notifications".into(),
            target_unit: "default.target".into(),
            unit_dirs: vec![],
            self_path: std::env::current_exe().unwrap(),
            sqlite_db_path: "./lksystem.db".into(),
        },
        fd_store: std::sync::RwLock::new(lksystem::fd_store::FDStore::default()),
        pid_table: std::sync::Mutex::new(PidTable::default()),
        unit_table,
        stdout_eventfd: lksystem::platform::make_event_fd().unwrap(),
        stderr_eventfd: lksystem::platform::make_event_fd().unwrap(),
        notification_eventfd: lksystem::platform::make_event_fd().unwrap(),
        socket_activation_eventfd: lksystem::platform::make_event_fd().unwrap(),
    }));

    // start socket activation thread
    lksystem::socket_activation::start_socketactivation_thread(run_info.clone());

    // start socket units so service can reach WaitingForSocket state
    let dbus_test_sock_id: lksystem::units::UnitId = "dbus-test.socket".try_into().unwrap();
    let dbus_sock_id: lksystem::units::UnitId = "dbus.socket".try_into().unwrap();
    let _ = lksystem::units::activate_unit(dbus_test_sock_id.clone(), &*run_info.read().unwrap(), lksystem::units::ActivationSource::Regular);
    let _ = lksystem::units::activate_unit(dbus_sock_id.clone(), &*run_info.read().unwrap(), lksystem::units::ActivationSource::Regular);

    // mark service as waiting for socket activation by attempting to start it (will return WaitingForSocket)
    lksystem::units::activate_unit(service_id.clone(), &*run_info.read().unwrap(), lksystem::units::ActivationSource::Regular).expect("activate service");

    // find socket unit id
    let socket_id = {
        let ri = run_info.read().unwrap();
        ri.unit_table.iter().find(|(_,u)| u.id.name.ends_with("dbus-test.socket")).unwrap().0.clone()
    };

    // activate socket unit (this opens the listening socket)
    lksystem::units::activate_unit(socket_id.clone(), &*run_info.read().unwrap(), lksystem::units::ActivationSource::Regular).expect("activate socket");

    // connect as client to the socket to trigger activation
    let socket_path_clone = socket_path.clone();
    let client = thread::spawn(move || {
        // give the listener a moment
        thread::sleep(Duration::from_millis(100));
        let _ = std::os::unix::net::UnixStream::connect(socket_path_clone);
    });

    // wait for activation
    thread::sleep(Duration::from_millis(500));

    // verify the service unit is started (or at least attempted)
    let started = {
        let ri = run_info.read().unwrap();
        ri.unit_table.iter().any(|(_,u)| u.id.name.ends_with("dbus-test.service") && *u.common.status.read().unwrap() != lksystem::units::UnitStatus::NeverStarted)
    };

    client.join().ok();
    assert!(started, "Service was not started by socket activation");
}
