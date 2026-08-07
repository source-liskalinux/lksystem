#[test]
fn test_unit_ordering() {
    let target1_str = format!(
        "
    [Unit]
    Description = {}
    Before = {}
    
    [Install]
    RequiredBy = {}
    ",
        "Target", "2.target", "2.target",
    );

    let parsed_file = crate::units::parse_file(&target1_str).unwrap();
    let target1_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/1.target"))
            .unwrap();

    let target2_str = format!(
        "
    [Unit]
    Description = {}
    After = {}

    [Install]
    RequiredBy = {}
    ",
        "Target", "1.target", "3.target",
    );

    let parsed_file = crate::units::parse_file(&target2_str).unwrap();
    let target2_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/2.target"))
            .unwrap();

    let target3_str = format!(
        "
    [Unit]
    Description = {}
    After = {}
    After = {}
    
    ",
        "Target", "1.target", "2.target"
    );

    let parsed_file = crate::units::parse_file(&target3_str).unwrap();
    let target3_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/3.target"))
            .unwrap();

    let mut unit_table = std::collections::HashMap::new();

    use crate::units::Unit;
    use std::convert::TryInto;
    let target1_unit: Unit = target1_unit.try_into().unwrap();
    let target2_unit: Unit = target2_unit.try_into().unwrap();
    let target3_unit: Unit = target3_unit.try_into().unwrap();
    let id1 = target1_unit.id.clone();
    let id2 = target2_unit.id.clone();
    let id3 = target3_unit.id.clone();

    unit_table.insert(target1_unit.id.clone(), target1_unit);
    unit_table.insert(target2_unit.id.clone(), target2_unit);
    unit_table.insert(target3_unit.id.clone(), target3_unit);

    crate::units::fill_dependencies(&mut unit_table).unwrap();
    unit_table
        .values_mut()
        .for_each(|unit| unit.dedup_dependencies());
    crate::units::sanity_check_dependencies(&unit_table).unwrap();

    unit_table
        .values()
        .for_each(|unit| crate::ui::write_line(format!("{} {:?}", unit.id, unit.common.dependencies)));

    // before/after 1.target
    assert!(unit_table
        .get(&id1)
        .unwrap()
        .common
        .dependencies
        .after
        .is_empty());
    assert!(
        unit_table
            .get(&id1)
            .unwrap()
            .common
            .dependencies
            .before
            .len()
            == 2
    );
    assert!(unit_table
        .get(&id1)
        .unwrap()
        .common
        .dependencies
        .before
        .contains(&id2));
    assert!(unit_table
        .get(&id1)
        .unwrap()
        .common
        .dependencies
        .before
        .contains(&id3));

    // before/after 2.target
    assert_eq!(
        unit_table
            .get(&id2)
            .unwrap()
            .common
            .dependencies
            .before
            .len(),
        1
    );
    assert!(unit_table
        .get(&id2)
        .unwrap()
        .common
        .dependencies
        .before
        .contains(&id3));
    assert_eq!(
        unit_table
            .get(&id2)
            .unwrap()
            .common
            .dependencies
            .after
            .len(),
        1
    );
    assert!(unit_table
        .get(&id2)
        .unwrap()
        .common
        .dependencies
        .after
        .contains(&id1));

    // before/after 3.target
    assert!(unit_table
        .get(&id3)
        .unwrap()
        .common
        .dependencies
        .before
        .is_empty());
    assert!(
        unit_table
            .get(&id3)
            .unwrap()
            .common
            .dependencies
            .after
            .len()
            == 2
    );
    assert!(unit_table
        .get(&id3)
        .unwrap()
        .common
        .dependencies
        .after
        .contains(&id2));
    assert!(unit_table
        .get(&id3)
        .unwrap()
        .common
        .dependencies
        .after
        .contains(&id1));

    // Test the collection of start subgraphs
    // add a new unrelated unit, that should never occur in any of these operations for {1,2,3}.target
    let target4_str = format!(
        "
    [Unit]
    Description = {}
    
    ",
        "Target"
    );
    let parsed_file = crate::units::parse_file(&target4_str).unwrap();
    let target4_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/4.target"))
            .unwrap();
    let target4_unit: Unit = target4_unit.try_into().unwrap();
    let id4 = target4_unit.id.clone();
    unit_table.insert(target4_unit.id.clone(), target4_unit);

    // 3.target needs all units
    let mut ids_to_start = vec![id3.clone()];
    crate::units::collect_unit_start_subgraph(&mut ids_to_start, &unit_table);
    ids_to_start.sort();
    assert_eq!(ids_to_start, vec![id1.clone(), id2.clone(), id3.clone()]);

    // 2.target needs 1 and 2
    let mut ids_to_start = vec![id2.clone()];
    crate::units::collect_unit_start_subgraph(&mut ids_to_start, &unit_table);
    ids_to_start.sort();
    assert_eq!(ids_to_start, vec![id1.clone(), id2.clone()]);

    // 1.target needs only 1
    let mut ids_to_start = vec![id1.clone()];
    crate::units::collect_unit_start_subgraph(&mut ids_to_start, &unit_table);
    ids_to_start.sort();
    assert_eq!(ids_to_start, vec![id1.clone()]);

    // 4.target needs only 4
    let mut ids_to_start = vec![id4.clone()];
    crate::units::collect_unit_start_subgraph(&mut ids_to_start, &unit_table);
    ids_to_start.sort();
    assert_eq!(ids_to_start, vec![id4.clone()]);
}

#[test]
fn test_wanted_by_install_relations() {
    let service_name = "test.service";
    let target_name = "default.target";

    let service_str = format!(
        "\n[Unit]\nDescription=Service\n[Install]\nWantedBy={}\n\n[Service]\nExecStart=/bin/true\n",
        target_name
    );
    let parsed_service = crate::units::parse_file(&service_str).unwrap();
    let service_unit = crate::units::parse_service(
        parsed_service,
        &std::path::PathBuf::from(format!("/path/to/{}", service_name)),
    )
    .unwrap();
    let service_unit: crate::units::Unit = service_unit.try_into().unwrap();

    let target_str = format!(
        "\n[Unit]\nDescription=Target\n\n[Install]\nRequiredBy={}\n",
        service_name
    );
    let parsed_target = crate::units::parse_file(&target_str).unwrap();
    let target_unit = crate::units::parse_target(
        parsed_target,
        &std::path::PathBuf::from(format!("/path/to/{}", target_name)),
    )
    .unwrap();
    let target_unit: crate::units::Unit = target_unit.try_into().unwrap();

    let mut unit_table = std::collections::HashMap::new();
    let service_id = service_unit.id.clone();
    let target_id = target_unit.id.clone();
    unit_table.insert(service_id.clone(), service_unit);
    unit_table.insert(target_id.clone(), target_unit);

    crate::units::fill_dependencies(&mut unit_table).unwrap();
    unit_table.values_mut().for_each(|unit| unit.dedup_dependencies());
    crate::units::sanity_check_dependencies(&unit_table).unwrap();

    assert!(unit_table
        .get(&target_id)
        .unwrap()
        .common
        .dependencies
        .wants
        .contains(&service_id));
    assert!(unit_table
        .get(&service_id)
        .unwrap()
        .common
        .dependencies
        .wanted_by
        .contains(&target_id));

    let mut ids_to_start = vec![target_id.clone()];
    crate::units::collect_unit_start_subgraph(&mut ids_to_start, &unit_table);
    ids_to_start.sort();
    let mut expected_ids = vec![service_id.clone(), target_id.clone()];
    expected_ids.sort();
    assert_eq!(ids_to_start, expected_ids);
}

#[test]
fn test_shared_socket_between_multiple_services() {
    let socket_name = "shared.socket";
    let service_a_name = "a.service";
    let service_b_name = "b.service";

    let socket_str = format!(
        "\n[Unit]\nDescription=Shared Socket\n[Socket]\nListenStream=/tmp/shared.sock\nService={}\nService={}\n",
        service_a_name, service_b_name
    );
    let parsed_socket = crate::units::parse_file(&socket_str).unwrap();
    let socket_unit = crate::units::parse_socket(
        parsed_socket,
        &std::path::PathBuf::from(format!("/path/to/{}", socket_name)),
    )
    .unwrap();
    let socket_unit: crate::units::Unit = socket_unit.try_into().unwrap();

    let service_a_str = format!(
        "\n[Unit]\nDescription=Service A\n[Service]\nExecStart=/bin/true\nSockets={}\n",
        socket_name
    );
    let parsed_service_a = crate::units::parse_file(&service_a_str).unwrap();
    let service_a_unit = crate::units::parse_service(
        parsed_service_a,
        &std::path::PathBuf::from(format!("/path/to/{}", service_a_name)),
    )
    .unwrap();
    let service_a_unit: crate::units::Unit = service_a_unit.try_into().unwrap();

    let service_b_str = format!(
        "\n[Unit]\nDescription=Service B\n[Service]\nExecStart=/bin/true\nSockets={}\n",
        socket_name
    );
    let parsed_service_b = crate::units::parse_file(&service_b_str).unwrap();
    let service_b_unit = crate::units::parse_service(
        parsed_service_b,
        &std::path::PathBuf::from(format!("/path/to/{}", service_b_name)),
    )
    .unwrap();
    let service_b_unit: crate::units::Unit = service_b_unit.try_into().unwrap();

    let mut unit_table = std::collections::HashMap::new();
    let socket_id = socket_unit.id.clone();
    let service_a_id = service_a_unit.id.clone();
    let service_b_id = service_b_unit.id.clone();
    unit_table.insert(socket_id.clone(), socket_unit);
    unit_table.insert(service_a_id.clone(), service_a_unit);
    unit_table.insert(service_b_id.clone(), service_b_unit);

    crate::units::fill_dependencies(&mut unit_table).unwrap();
    unit_table.values_mut().for_each(|unit| unit.dedup_dependencies());
    crate::units::sanity_check_dependencies(&unit_table).unwrap();

    let socket_unit = unit_table.get(&socket_id).unwrap();
    if let crate::units::Specific::Socket(sock) = &socket_unit.specific {
        assert!(sock.conf.services.contains(&service_a_id));
        assert!(sock.conf.services.contains(&service_b_id));
    } else {
        panic!("Expected socket unit");
    }

    let service_a_unit = unit_table.get(&service_a_id).unwrap();
    let service_b_unit = unit_table.get(&service_b_id).unwrap();
    if let crate::units::Specific::Service(srvc) = &service_a_unit.specific {
        assert!(srvc.conf.sockets.contains(&socket_id));
    } else {
        panic!("Expected service A unit");
    }
    if let crate::units::Specific::Service(srvc) = &service_b_unit.specific {
        assert!(srvc.conf.sockets.contains(&socket_id));
    } else {
        panic!("Expected service B unit");
    }
}

#[test]
fn test_fill_dependencies_reports_unknown_units() {
    let target_str = "\n[Unit]\nDescription=Target\nAfter=missing.target\n";
    let parsed_target = crate::units::parse_file(target_str).unwrap();
    let target_unit = crate::units::parse_target(
        parsed_target,
        &std::path::PathBuf::from("/path/to/default.target"),
    )
    .unwrap();
    let target_unit: crate::units::Unit = target_unit.try_into().unwrap();

    let mut unit_table = std::collections::HashMap::new();
    unit_table.insert(target_unit.id.clone(), target_unit);

    let result = crate::units::fill_dependencies(&mut unit_table);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("unknown unit"));
}

#[test]
fn test_fill_dependencies_reports_unknown_requires_wants_units() {
    let service_str = "\n[Unit]\nDescription=Service\nWants=missing.service\nRequires=missing2.service\n[Service]\nExecStart=/bin/true\n";
    let parsed_service = crate::units::parse_file(service_str).unwrap();
    let service_unit = crate::units::parse_service(
        parsed_service,
        &std::path::PathBuf::from("/path/to/test.service"),
    )
    .unwrap();
    let service_unit: crate::units::Unit = service_unit.try_into().unwrap();

    let mut unit_table = std::collections::HashMap::new();
    unit_table.insert(service_unit.id.clone(), service_unit);

    let result = crate::units::fill_dependencies(&mut unit_table);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("unknown unit"));
    assert!(err.contains("missing.service") || err.contains("missing2.service"));
}

#[test]
fn test_load_all_units_reports_unknown_target() {
    let dir = tempfile::tempdir().unwrap();
    let service_path = dir.path().join("test.service");
    std::fs::write(
        &service_path,
        "\n[Unit]\nDescription=Service\n[Service]\nExecStart=/bin/true\n",
    )
    .unwrap();

    let result = crate::units::load_all_units(&[dir.path().to_path_buf()], "missing.target");
    assert!(result.is_err());
    if let Err(crate::units::LoadingError::Dependency(err)) = result {
        assert!(err.to_string().contains("Target unit missing.target not found"));
    } else {
        panic!("Expected dependency error for missing target");
    }
}

#[test]
fn test_circle() {
    let target1_str = format!(
        "
    [Unit]
    Description = {}
    After = {}
    ",
        "Target", "3.target"
    );

    let parsed_file = crate::units::parse_file(&target1_str).unwrap();
    let target1_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/1.target"))
            .unwrap();

    let target2_str = format!(
        "
    [Unit]
    Description = {}
    After = {}
    ",
        "Target", "1.target"
    );

    let parsed_file = crate::units::parse_file(&target2_str).unwrap();
    let target2_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/2.target"))
            .unwrap();

    let target3_str = format!(
        "
    [Unit]
    Description = {}
    After = {}
    ",
        "Target", "2.target"
    );

    let parsed_file = crate::units::parse_file(&target3_str).unwrap();
    let target3_unit =
        crate::units::parse_target(parsed_file, &std::path::PathBuf::from("/path/to/3.target"))
            .unwrap();

    use crate::units::Unit;
    use std::convert::TryInto;
    let mut unit_table = std::collections::HashMap::new();
    let target1_unit: Unit = target1_unit.try_into().unwrap();
    let target2_unit: Unit = target2_unit.try_into().unwrap();
    let target3_unit: Unit = target3_unit.try_into().unwrap();
    let target1_id = target1_unit.id.clone();
    let target2_id = target2_unit.id.clone();
    let target3_id = target3_unit.id.clone();
    unit_table.insert(target1_unit.id.clone(), target1_unit);
    unit_table.insert(target2_unit.id.clone(), target2_unit);
    unit_table.insert(target3_unit.id.clone(), target3_unit);

    crate::units::fill_dependencies(&mut unit_table).unwrap();
    unit_table
        .values_mut()
        .for_each(|unit| unit.dedup_dependencies());

    if let Err(crate::units::SanityCheckError::CirclesFound(circles)) =
        crate::units::sanity_check_dependencies(&unit_table)
    {
        if circles.len() == 1 {
            let circle = &circles[0];
            assert_eq!(circle.len(), 3);
            assert!(circle.contains(&target1_id));
            assert!(circle.contains(&target2_id));
            assert!(circle.contains(&target3_id));
        } else {
            panic!("more than one circle found but there is only one");
        }
    } else {
        panic!("No circle found but there is one");
    }
}
