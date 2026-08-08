//! Wait for sockets to activate their respective services
use crate::ui;

use crate::runtime_info::*;
use crate::units::*;
use std::os::unix::io::BorrowedFd;

pub fn start_socketactivation_thread(run_info: ArcMutRuntimeInfo) {
    std::thread::spawn(move || loop {
        let wait_result = wait_for_socket(run_info.clone());
        match wait_result {
            Ok(ids) => {
                let run_info = run_info.read().unwrap();
                let unit_table = &run_info.unit_table;
                for socket_id in ids {
                    let mut matching_services = Vec::new();
                    for unit in unit_table.values() {
                        if let crate::units::Specific::Service(specific) = &unit.specific {
                            if specific.has_socket(&socket_id.name) {
                                matching_services.push(unit.id.clone());
                            }
                        }
                    }

                    if matching_services.is_empty() {
                        ui::error(format!(
                            "Socket unit {:?} activated, but the service could not be found",
                            socket_id
                        ));
                        continue;
                    }

                    let mut still_waiting = false;
                    let mut activation_attempted = false;

                    for service_id in matching_services {
                        let service_unit = unit_table.get(&service_id).unwrap();
                        let srvc_status = {
                            let status_locked = &*service_unit.common.status.read().unwrap();
                            status_locked.clone()
                        };

                        if srvc_status != UnitStatus::Started(StatusStarted::WaitingForSocket) {
                            ui::log(format!(
                                "Ignore socket activation for service {} because status is {:?}",
                                service_id.name,
                                srvc_status
                            ));
                            continue;
                        }

                        activation_attempted = true;
                        match crate::units::activate_unit(
                            service_id.clone(),
                            &*run_info,
                            ActivationSource::SocketActivation,
                        ) {
                            Ok(_) => {
                                ui::log(format!(
                                    "Service {} started via shared socket activation",
                                    service_id.name
                                ));
                            }
                            Err(e) => {
                                match &e.reason {
                                    crate::units::UnitOperationErrorReason::DependencyError(_) => {
                                        ui::log(format!(
                                            "Delayed socket activation for service {} because dependencies are not ready: {}",
                                            service_id.name,
                                            e
                                        ));
                                        still_waiting = true;
                                    }
                                    _ => {
                                        ui::error(format!(
                                            "Error while starting service from socket activation: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    if activation_attempted && !still_waiting {
                        let sock_unit = unit_table.get(&socket_id).unwrap();
                        if let Specific::Socket(specific) = &sock_unit.specific {
                            let mut_state = &mut *specific.state.write().unwrap();
                            mut_state.sock.activated = true;
                        }
                    }
                }
            }
            Err(e) => {
                ui::error(format!("Error in socket activation loop: {}", e));
                break;
            }
        }
    });
}

pub fn wait_for_socket(run_info: ArcMutRuntimeInfo) -> Result<Vec<UnitId>, String> {
    let eventfd = { run_info.read().unwrap().socket_activation_eventfd };
    let (mut fdset, fd_to_sock_id) = {
        let run_info_locked = &*run_info.read().unwrap();

        let fd_to_sock_id = run_info_locked.fd_store.read().unwrap().global_fds_to_ids();
        let mut fdset = nix::sys::select::FdSet::new();
        {
            let unit_table_locked = &run_info_locked.unit_table;
            for (fd, id) in &fd_to_sock_id {
                let unit = unit_table_locked.get(id).unwrap();
                if let Specific::Socket(specific) = &unit.specific {
                    let mut_state = &*specific.state.read().unwrap();
                    if !mut_state.sock.activated {
                        fdset.insert(unsafe { BorrowedFd::borrow_raw(*fd) });
                    }
                }
            }
            fdset.insert(unsafe { BorrowedFd::borrow_raw(eventfd.read_end()) });
        }
        (fdset, fd_to_sock_id)
    };

    let result = nix::sys::select::select(None, Some(&mut fdset), None, None, None);
    match result {
        Ok(_) => {
            let mut activated_ids = Vec::new();
            if fdset.contains(unsafe { BorrowedFd::borrow_raw(eventfd.read_end()) }) {
                ui::log(format!("Interrupted socketactivation select because the eventfd fired"));
                crate::platform::reset_event_fd(eventfd);
                ui::log(format!("Reset eventfd value"));
            } else {
                for (fd, id) in &fd_to_sock_id {
                    if fdset.contains(unsafe { BorrowedFd::borrow_raw(*fd) }) {
                        activated_ids.push(id.clone());
                    }
                }
            }
            Ok(activated_ids)
        }
        Err(e) => {
            if let nix::Error::EINTR = e {
                Ok(Vec::new())
            } else {
                Err(format!("Error while selecting: {}", e))
            }
        }
    }
}
