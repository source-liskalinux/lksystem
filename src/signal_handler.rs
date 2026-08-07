//! Handle signals send to this process from either the outside or the child processes

use crate::runtime_info::*;
use crate::services;
use lksystem::ui;
use signal_hook::iterator::Signals;

pub fn handle_signals(mut signals: Signals, run_info: ArcMutRuntimeInfo) {
    loop {
        // Pick up new signals
        for signal in signals.forever() {
            match signal as libc::c_int {
                signal_hook::consts::SIGCHLD => {
                    std::iter::from_fn(get_next_exited_child)
                        .take_while(Result::is_ok)
                        .for_each(|val| {
                            let run_info_clone = run_info.clone();
                            match val {
                                Ok((pid, code)) => services::service_exit_handler_new_thread(
                                    pid,
                                    code,
                                    run_info_clone,
                                ),
                                Err(e) => {
                                    ui::error(format!("{}", e));
                                }
                            }
                        });
                }
                signal_hook::consts::SIGTERM
                | signal_hook::consts::SIGINT
                | signal_hook::consts::SIGQUIT => {
                    ui::log(format!("Received termination signal. lksystem checking out"));
                    crate::shutdown::shutdown_sequence(
                        run_info.clone(),
                        crate::shutdown::ShutdownAction::Poweroff,
                    );
                }

                _ => unreachable!(),
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum ChildTermination {
    Signal(nix::sys::signal::Signal),
    Exit(i32),
}

impl ChildTermination {
    pub fn success(&self) -> bool {
        match self {
            ChildTermination::Signal(_) => false,
            ChildTermination::Exit(code) => *code == 0,
        }
    }
}

type ChildIterElem = Result<(nix::unistd::Pid, ChildTermination), nix::Error>;

fn get_next_exited_child() -> Option<ChildIterElem> {
    let wait_any_pid = nix::unistd::Pid::from_raw(-1);
    let wait_flags = nix::sys::wait::WaitPidFlag::WNOHANG;
    match nix::sys::wait::waitpid(wait_any_pid, Some(wait_flags)) {
        Ok(exit_status) => match exit_status {
            nix::sys::wait::WaitStatus::Exited(pid, code) => {
                Some(Ok((pid, ChildTermination::Exit(code))))
            }
            nix::sys::wait::WaitStatus::Signaled(pid, signal, _dumped_core) => {
                // signals get handed to the parent if the child got killed by it but didnt handle the
                // signal itself
                // we dont care if the service dumped it's core
                Some(Ok((pid, ChildTermination::Signal(signal))))
            }
            nix::sys::wait::WaitStatus::StillAlive => {
                ui::log(format!("No more state changes to poll"));
                None
            }
            _ => {
                ui::log(format!("Ignored child signal received with code: {:?}", exit_status));
                // return next child, we dont care about other events like stop/continue of children
                get_next_exited_child()
            }
        },
        Err(e) => {
            if let nix::Error::ECHILD = e {
            } else {
                ui::log(format!("Error while waiting: {}", e));
            }
            Some(Err(e))
        }
    }
}
