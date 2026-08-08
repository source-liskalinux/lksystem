use crate::ui;

use crate::runtime_info::*;
use crate::units::*;

/// Apa yang harus dilakukan lksystem SETELAH semua unit berhasil dimatikan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownAction {
    Poweroff,
    Reboot,
    Halt,
    /// Cuma keluar dari proses (dipakai kalau bukan PID1: dev-mode / service
    /// manager biasa di dalam container tanpa init=).
    ExitOnly,
}

fn get_next_service_to_shutdown(unit_table: &UnitTable) -> Option<UnitId> {
    for (_, unit) in unit_table.iter() {
        let status = &unit.common.status;
        {
            let status_locked = status.read().unwrap();
            if !(*status_locked).is_started() {
                continue;
            }
        }

        let kill_before = unit
            .common
            .dependencies
            .before
            .iter()
            .cloned()
            .filter(|next_id| {
                let unit = unit_table.get(next_id).unwrap();
                let status = &unit.common.status;
                let status_locked = status.read().unwrap();
                status_locked.is_started()
            })
            .collect::<Vec<_>>();
        if kill_before.is_empty() {
            ui::log(format!("Chose unit: {}", unit.id.name));
            return Some(unit.id.clone());
        } else {
            ui::log(format!(
                "Dont kill service {} yet. These Units depend on it: {:?}",
                unit.id.name,
                kill_before
            ));
        }
    }
    None
}

fn shutdown_unit(shutdown_id: &UnitId, run_info: &RuntimeInfo) {
    let unit = run_info.unit_table.get(shutdown_id).unwrap();
    {
        ui::log(format!("Set unit status: {}", unit.id.name));
        let mut status_locked = unit.common.status.write().unwrap();
        *status_locked = UnitStatus::Stopping;
    }
    match &unit.specific {
        Specific::Service(specific) => {
            let mut_state = &mut *specific.state.write().unwrap();
            let kill_res =
                mut_state
                    .srvc
                    .kill(&specific.conf, unit.id.clone(), &unit.id.name, run_info);
            match kill_res {
                Ok(()) => {
                    ui::log(format!("Killed service unit: {}", unit.id.name));
                }
                Err(e) => ui::error(format!("{}", e)),
            }
            if let Some(datagram) = &mut_state.srvc.notifications {
                match datagram.shutdown(std::net::Shutdown::Both) {
                    Ok(()) => {
                        ui::log(format!(
                            "Closed notification socket for service unit: {}",
                            unit.id.name
                        ));
                    }
                    Err(e) => ui::error(format!(
                        "Error closing notification socket for service unit {}: {}",
                        unit.id.name, e
                    )),
                }
            }
            mut_state.srvc.notifications = None;

            if let Some(note_sock_path) = &mut_state.srvc.notifications_path {
                if note_sock_path.exists() {
                    match std::fs::remove_file(note_sock_path) {
                        Ok(()) => {
                            ui::log(format!(
                                "Removed notification socket for service unit: {}",
                                unit.id.name
                            ));
                        }
                        Err(e) => ui::error(format!(
                            "Error removing notification socket for service unit {}: {}",
                            unit.id.name, e
                        )),
                    }
                }
            }
        }
        Specific::Socket(specific) => {
            let mut_state = &mut *specific.state.write().unwrap();
            ui::log(format!("Close socket unit: {}", unit.id.name));
            match mut_state.sock.close_all(
                &specific.conf,
                unit.id.name.clone(),
                &mut *run_info.fd_store.write().unwrap(),
            ) {
                Err(e) => ui::error(format!("Error while closing sockets: {}", e)),
                Ok(()) => {}
            }
            ui::log(format!("Closed socket unit: {}", unit.id.name));
        }
        Specific::Target(_) => {
            // Nothing to do
        }
        Specific::Device(_) => {
            // Nothing to do: no process/fd of our own to tear down. Status is set
            // to Stopped below same as every other unit kind.
        }
        Specific::Timer(_) => {
            // Drop any pending wakeup so the scheduler thread doesn't try to
            // activate something after we've already torn the unit graph
            // down.
            crate::timer_events::cancel(&unit.id);
        }
        Specific::Mount(specific) => {
            let mut_state = &mut *specific.state.write().unwrap();
            let _ = mut_state.deactivate(&unit.id, &specific.conf, &unit.common.status);
        }
        Specific::Path(_) => {
            // Nothing to do for .path units on shutdown.
        }
    }
    {
        ui::log(format!("Set unit status: {}", unit.id.name));
        let mut status_locked = unit.common.status.write().unwrap();
        *status_locked = UnitStatus::Stopped(StatusStopped::StoppedFinal, vec![]);
    }
}

static SHUTTING_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
// TODO maybe this should be available everywhere for situations where normally a panic would occur?
pub fn shutdown_sequence(run_info: ArcMutRuntimeInfo, action: ShutdownAction) {
    if SHUTTING_DOWN
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        ui::warning(format!("Got a second termination signal. Exiting potentially dirty"));
        finalize_shutdown(action);
        return;
    }

    std::thread::spawn(move || {
        ui::log(format!("Shutting down"));
        let run_info_lock = match run_info.read() {
            Ok(r) => r,
            Err(e) => e.into_inner(),
        };
        let run_info_locked = &*run_info_lock;

        ui::log(format!("Kill all units"));
        loop {
            let id = {
                if let Some(id) = get_next_service_to_shutdown(&run_info_locked.unit_table) {
                    id
                } else {
                    break;
                }
            };
            shutdown_unit(&id, run_info_locked);
        }
        ui::log(format!("Killed all units"));

        let control_socket = run_info_locked
            .config
            .notification_sockets_dir
            .join("control.socket");
        if control_socket.exists() {
            match std::fs::remove_file(control_socket) {
                Ok(()) => {
                    ui::log(format!("Removed control socket"));
                }
                Err(e) => ui::error(format!("Error removing control socket: {}", e)),
            }
        }

        #[cfg(target_os = "linux")]
        {
            let _ = crate::platform::cgroups::move_out_of_own_cgroup(&std::path::PathBuf::from(
                "/sys/fs/cgroup/unified",
            ))
            .map_err(|e| ui::error(format!("Error while cleaning up cgroups: {}", e)));
        }

        ui::log(format!("Shutdown finished"));
        finalize_shutdown(action);
    });
}

/// BUG LAMA FATAL: shutdown selalu memanggil `std::process::exit(0)` apa pun
/// kondisinya. Kalau proses ini benar-benar PID1, exit() di init process
/// bikin KERNEL PANIC ("Attempted to kill init!") -- PID1 wajib memanggil
/// syscall reboot(2) sendiri (RB_POWER_OFF/RB_AUTOBOOT/dst), tidak pernah
/// boleh sekadar keluar dari proses seperti proses biasa.
fn finalize_shutdown(action: ShutdownAction) {
    let _ = nix::unistd::sync();
    let is_pid1 = nix::unistd::getpid().as_raw() == 1;

    if action == ShutdownAction::ExitOnly || !is_pid1 {
        if is_pid1 {
            ui::warning(format!("PID1 diminta ExitOnly, tidak aman -- reboot sebagai gantinya"));
            do_reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT);
        }
        ui::log(format!("Keluar dari proses lksystem (bukan PID1, tidak mematikan/reboot mesin)"));
        std::process::exit(0);
    }

    let mode = match action {
        ShutdownAction::Poweroff => nix::sys::reboot::RebootMode::RB_POWER_OFF,
        ShutdownAction::Reboot => nix::sys::reboot::RebootMode::RB_AUTOBOOT,
        ShutdownAction::Halt => nix::sys::reboot::RebootMode::RB_HALT_SYSTEM,
        ShutdownAction::ExitOnly => unreachable!("sudah ditangani di atas"),
    };
    do_reboot(mode);
}

fn do_reboot(mode: nix::sys::reboot::RebootMode) -> ! {
    match nix::sys::reboot::reboot(mode) {
        Ok(_) => unreachable!("reboot() sukses tidak pernah kembali ke caller"),
        Err(e) => {
            ui::error(format!("Syscall reboot() gagal: {} -- exit paksa sebagai fallback darurat", e));
            std::process::exit(1);
        }
    }
}
