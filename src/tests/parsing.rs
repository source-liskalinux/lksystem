use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_service_parsing() {
    let descr = "This is a description";
    let unit_before1 = "unit_before2";
    let unit_before2 = "unit_before1";
    let unit_after1 = "unit_after1";
    let unit_after2 = "unit_after2,unit_after3";

    let install_required_by = "install_req_by";
    let install_wanted_by = "install_wanted_by";

    let service_execstart = "/path/to/startbin arg1 arg2 arg3";
    let service_execpre = "--/path/to/startprebin arg1 arg2 arg3";
    let service_execpost = "/path/to/startpostbin arg1 arg2 arg3";
    let service_stop = "/path/to/stopbin arg1 arg2 arg3";
    let service_sockets = "socket_name1,socket_name2";
    let service_workdir = "/var/lib/example";
    let service_environment = "FOO=bar BAZ=qux";
    let service_envfile = "/etc/default/example";
    let service_reload = "/bin/true";
    let service_restartsec = "5s";
    let unit_conflict = "other.service";
    let condition_path_exists = "/tmp/required-file";
    let condition_path_is_directory = "/var/lib/example";

    let test_service_str = format!(
        r#"
    [Unit]
    Description = {}
    Before = {}
    Before = {}
    After = {}
    After = {}
    Conflicts = {}

    [Condition]
    ConditionPathExists = {}
    ConditionPathIsDirectory = {}
    
    [Install]
    RequiredBy = {}
    WantedBy = {}
    
    [Service]
    ExecStart = {}
    ExecStartPre = {}
    ExecStartPost = {}
    ExecStop = {}
    Sockets = {}
    WorkingDirectory = {}
    Environment = {}
    EnvironmentFile = {}
    ExecReload = {}
    RemainAfterExit = yes
    Restart = always
    RestartSec = {}

    "#,
        descr,
        unit_before1,
        unit_before2,
        unit_after1,
        unit_after2,
        unit_conflict,
        condition_path_exists,
        condition_path_is_directory,
        install_required_by,
        install_wanted_by,
        service_execstart,
        service_execpre,
        service_execpost,
        service_stop,
        service_sockets,
        service_workdir,
        service_environment,
        service_envfile,
        service_reload,
        service_restartsec,
    );

    let parsed_file = crate::units::parse_file(&test_service_str).unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/unitfile.service"),
    )
    .unwrap();

    // check all the values

    assert_eq!(service.common.unit.description, descr);
    assert_eq!(
        service.common.unit.before,
        vec![unit_before1.to_owned(), unit_before2.to_owned()]
    );
    assert_eq!(
        service.common.unit.after,
        vec![
            unit_after1.to_owned(),
            "unit_after2".to_owned(),
            "unit_after3".to_owned()
        ]
    );
    assert_eq!(service.common.unit.conflicts, vec![unit_conflict.to_owned()]);
    assert_eq!(
        service.common.conditions.path_exists,
        vec![std::path::PathBuf::from(condition_path_exists)]
    );
    assert_eq!(
        service.common.conditions.path_is_directory,
        vec![std::path::PathBuf::from(condition_path_is_directory)]
    );

    assert_eq!(
        service.common.install.required_by,
        vec![install_required_by.to_owned()]
    );
    assert_eq!(
        service.common.install.wanted_by,
        vec![install_wanted_by.to_owned()]
    );

    assert_eq!(
        service.srvc.exec,
        crate::units::Commandline {
            cmd: "/path/to/startbin".into(),
            args: vec!["arg1".into(), "arg2".into(), "arg3".into()],
            prefixes: vec![],
        }
    );
    assert_eq!(
        service.srvc.startpre,
        vec![crate::units::Commandline {
            cmd: "/path/to/startprebin".into(),
            args: vec!["arg1".into(), "arg2".into(), "arg3".into()],
            prefixes: vec![
                crate::units::CommandlinePrefix::Minus,
                crate::units::CommandlinePrefix::Minus,
            ],
        }]
    );
    assert_eq!(
        service.srvc.startpost,
        vec![crate::units::Commandline {
            cmd: "/path/to/startpostbin".into(),
            args: vec!["arg1".into(), "arg2".into(), "arg3".into()],
            prefixes: vec![],
        }]
    );
    assert_eq!(
        service.srvc.stop,
        vec![crate::units::Commandline {
            cmd: "/path/to/stopbin".into(),
            args: vec!["arg1".into(), "arg2".into(), "arg3".into()],
            prefixes: vec![],
        }]
    );
    assert_eq!(
        service.srvc.sockets,
        vec!["socket_name1".to_owned(), "socket_name2".to_owned()]
    );
    assert_eq!(
        service.srvc.exec_section.working_directory,
        Some(std::path::PathBuf::from(service_workdir))
    );

    let service_dbus_str = format!(
        "\n[Unit]\nDescription=DBus Service\n[Service]\nType=dbus\nBusName={}\nExecStart=/bin/true\n",
        "org.example.Service"
    );

    let parsed_dbus_service = crate::units::parse_file(&service_dbus_str).unwrap();
    let dbus_service_unit = crate::units::parse_service(
        parsed_dbus_service,
        &std::path::PathBuf::from("/path/to/dbus.service"),
    )
    .unwrap();
    let dbus_service_unit: crate::units::Unit = dbus_service_unit.try_into().unwrap();
    if let crate::units::Specific::Service(specific) = &dbus_service_unit.specific {
        assert_eq!(specific.conf.srcv_type, crate::units::ServiceType::Dbus);
        assert_eq!(specific.conf.dbus_name.as_deref(), Some("org.example.Service"));
    } else {
        panic!("Expected a Service unit");
    }
    assert_eq!(
        service.srvc.exec_section.environment,
        Some(crate::units::EnvVars {
            vars: vec![
                ("FOO".to_owned(), "bar".to_owned()),
                ("BAZ".to_owned(), "qux".to_owned()),
            ],
        })
    );
    assert_eq!(
        service.srvc.exec_section.environment_files,
        vec![std::path::PathBuf::from(service_envfile)]
    );
    assert_eq!(
        service.srvc.reload,
        Some(crate::units::Commandline {
            cmd: "/bin/true".into(),
            args: vec![],
            prefixes: vec![],
        })
    );
    assert!(service.srvc.remain_after_exit);
    assert_eq!(
        service.srvc.restart_sec,
        Some(std::time::Duration::from_secs(5))
    );
}

#[test]
fn test_systemd_drop_in_overrides_are_merged() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("lksystem-dropin-{unique}"));
    fs::create_dir_all(&temp_dir).unwrap();

    let unit_path = temp_dir.join("example.service");
    fs::write(
        &unit_path,
        "[Unit]\nDescription=Base unit\n\n[Service]\nExecStart=/bin/echo base\n",
    )
    .unwrap();

    let override_dir = temp_dir.join("example.service.d");
    fs::create_dir_all(&override_dir).unwrap();
    fs::write(
        override_dir.join("10-override.conf"),
        "[Unit]\nDescription=Overridden unit\n\n[Service]\nExecStart=/bin/echo override\nRestart=always\n",
    )
    .unwrap();

    let parsed_file = crate::units::parse_unit_file(&unit_path).unwrap();
    crate::ui::write_line(format!("parsed_file: {:#?}", parsed_file));
    let service = crate::units::parse_service(parsed_file, &unit_path).unwrap();

    assert_eq!(service.common.unit.description, "Overridden unit");
    assert_eq!(service.srvc.exec.cmd, "/bin/echo");
    assert_eq!(service.srvc.exec.args, vec!["override"]);
    assert_eq!(service.srvc.restart, crate::units::ServiceRestart::Always);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_restart_policy_parsing_supports_on_failure() {
    let parsed_file = crate::units::parse_file(
        "\n[Service]\nType=oneshot\nRestart=on-failure\nExecStart=/bin/true\n",
    )
    .unwrap();
    let service = crate::units::parse_service(
        parsed_file,
        &std::path::PathBuf::from("/path/to/restart.service"),
    )
    .unwrap();

    assert_eq!(service.srvc.restart, crate::units::ServiceRestart::OnFailure);
}

#[test]
fn test_mount_parsing() {
    let test_mount_str = r#"
    [Unit]
    Description = Mount an image

    [Mount]
    What = /dev/loop0
    Where = /mnt/image
    Type = ext4
    Options = ro

    [Install]
    WantedBy = multi-user.target
    "#;

    let parsed_file = crate::units::parse_file(test_mount_str).unwrap();
    let mount = crate::units::parse_mount(
        parsed_file,
        &std::path::PathBuf::from("/path/to/test.mount"),
    )
    .unwrap();

    let unit: crate::units::Unit = mount.try_into().unwrap();
    assert_eq!(unit.id.name, "test.mount");
    assert!(unit.is_mount());
    if let crate::units::Specific::Mount(mount_specific) = &unit.specific {
        assert_eq!(mount_specific.conf.what, std::path::PathBuf::from("/dev/loop0"));
        assert_eq!(mount_specific.conf.where_path, std::path::PathBuf::from("/mnt/image"));
        assert_eq!(mount_specific.conf.fstype.as_deref(), Some("ext4"));
        assert_eq!(mount_specific.conf.options.as_deref(), Some("ro"));
        assert!(!mount_specific.state.read().unwrap().mounted);
    } else {
        panic!("Expected a Mount unit");
    }
}

#[test]
fn test_socket_parsing() {
    let descr = "This is a description";
    let unit_before1 = "unit_before2";
    let unit_before2 = "unit_before1";
    let unit_after1 = "unit_after1";
    let unit_after2 = "unit_after2,unit_after3";

    let install_required_by = "install_req_by";
    let install_wanted_by = "install_wanted_by";

    let socket_fdname = "SocketyMcSockface";
    let socket_ipv4 = "127.0.0.1:8080";
    let socket_ipv6 = "[fe81::]:8080";
    let socket_unix = "/path/to/socket";
    let socket_service = "other_name";

    let test_service_str = format!(
        r#"
    [Unit]
    Description = {}
    Before = {}
    Before = {}
    After = {}
    After = {}
    
    [Install]
    RequiredBy = {}
    WantedBy = {}
    
    [Socket]
    ListenStream = {}
    ListenStream = {}
    ListenStream = {}

    ListenDatagram = {}
    ListenDatagram = {}
    ListenDatagram = {}

    ListenSequentialPacket = {}
    ListenFifo = {}
    Service= {}
    FileDescriptorName= {}

    "#,
        descr,
        unit_before1,
        unit_before2,
        unit_after1,
        unit_after2,
        install_required_by,
        install_wanted_by,
        socket_ipv4,
        socket_ipv6,
        socket_unix,
        socket_ipv4,
        socket_ipv6,
        socket_unix,
        socket_unix,
        socket_unix,
        socket_service,
        socket_fdname,
    );

    let parsed_file = crate::units::parse_file(&test_service_str).unwrap();
    let socket_unit = crate::units::parse_socket(
        parsed_file,
        &std::path::PathBuf::from("/path/to/unitfile.socket"),
    )
    .unwrap();

    // check all the values

    assert_eq!(socket_unit.common.unit.description, descr);
    assert_eq!(
        socket_unit.common.unit.before,
        vec![unit_before1.to_owned(), unit_before2.to_owned()]
    );
    assert_eq!(
        socket_unit.common.unit.after,
        vec![
            unit_after1.to_owned(),
            "unit_after2".to_owned(),
            "unit_after3".to_owned()
        ]
    );

    assert_eq!(
        socket_unit.common.install.required_by,
        vec![install_required_by.to_owned()]
    );
    assert_eq!(
        socket_unit.common.install.wanted_by,
        vec![install_wanted_by.to_owned()]
    );
    if socket_unit.sock.sockets.len() == 8 {
        // streaming sockets
        if let crate::sockets::SpecializedSocketConfig::TcpSocket(tcpconf) =
            &socket_unit.sock.sockets[0].specialized
        {
            if !tcpconf.addr.is_ipv4() {
                panic!("Should have been an ipv4 address but wasnt");
            }
        } else {
            panic!("Sockets[0] should have been a tcp socket, but wasnt");
        }
        if let crate::sockets::SpecializedSocketConfig::TcpSocket(tcpconf) =
            &socket_unit.sock.sockets[1].specialized
        {
            if !tcpconf.addr.is_ipv6() {
                panic!("Should have been an ipv6 address but wasnt");
            }
        } else {
            panic!("Sockets[1] should have been a tcp socket, but wasnt");
        }
        if let crate::sockets::SpecializedSocketConfig::UnixSocket(
            crate::sockets::UnixSocketConfig::Stream(addr),
        ) = &socket_unit.sock.sockets[2].specialized
        {
            assert_eq!(addr, socket_unix);
        } else {
            panic!("Sockets[2] should have been a streaming unix socket, but wasnt");
        }

        // Datagram sockets
        if let crate::sockets::SpecializedSocketConfig::UdpSocket(tcpconf) =
            &socket_unit.sock.sockets[3].specialized
        {
            if !tcpconf.addr.is_ipv4() {
                panic!("Should have been an ipv4 address but wasnt");
            }
        } else {
            panic!("Sockets[3] should have been a udp socket, but wasnt");
        }
        if let crate::sockets::SpecializedSocketConfig::UdpSocket(tcpconf) =
            &socket_unit.sock.sockets[4].specialized
        {
            if !tcpconf.addr.is_ipv6() {
                panic!("Should have been an ipv6 address but wasnt");
            }
        } else {
            panic!("Sockets[4] should have been a udp socket, but wasnt");
        }
        if let crate::sockets::SpecializedSocketConfig::UnixSocket(
            crate::sockets::UnixSocketConfig::Datagram(addr),
        ) = &socket_unit.sock.sockets[5].specialized
        {
            assert_eq!(addr, socket_unix);
        } else {
            panic!("Sockets[5] should have been a datagram unix socket, but wasnt");
        }

        // SeqPacket socket
        if let crate::sockets::SpecializedSocketConfig::UnixSocket(
            crate::sockets::UnixSocketConfig::Sequential(addr),
        ) = &socket_unit.sock.sockets[6].specialized
        {
            assert_eq!(addr, socket_unix);
        } else {
            panic!("Sockets[6] should have been a sequential packet unix socket, but wasnt");
        }
        // SeqPacket socket
        if let crate::sockets::SpecializedSocketConfig::Fifo(fifoconf) =
            &socket_unit.sock.sockets[7].specialized
        {
            assert_eq!(fifoconf.path, std::path::PathBuf::from(socket_unix));
        } else {
            panic!("Sockets[6] should have been a sequential packet unix socket, but wasnt");
        }
    } else {
        panic!("Not enough sockets parsed");
    }
}
