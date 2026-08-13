use lksystem::core::{install_signal_handlers, take_terminate, SIGTERM};
use lksystem::ui;
use std::env;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

unsafe extern "C" {
    fn getpid() -> i32;
    fn setsid() -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn sync();
    fn reboot(command: i32) -> i32;
}

const RB_AUTOBOOT: i32 = 0x0123_4567;
const RB_POWER_OFF: i32 = 0x4321_fedc;
const STAGES: [&str; 3] = ["/etc/lksystem/1", "/etc/lksystem/2", "/etc/lksystem/3"];

fn start_stage(path: &str) -> io::Result<std::process::Child> {
    Command::new(path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .process_group(0)
        .spawn()
}

fn main() -> io::Result<()> {
    if unsafe { getpid() } != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "lksystem must run as PID 1",
        ));
    }
    unsafe { setsid() };
    install_signal_handlers();
    ui::welcome();
    ui::log("Initializing lksystem....");
    let mut shutdown_requested = false;
    for (index, stage) in STAGES.iter().enumerate() {
        loop {
            let mut child = start_stage(stage)?;
            loop {
                if let Some(status) = child.try_wait()? {
                    if index == 1 && !status.success() {
                        break;
                    }
                    break;
                }
                if index == 1 && take_terminate() {
                    unsafe { kill(child.id() as i32, SIGTERM) };
                    let _ = child.wait();
                    shutdown_requested = true;
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            if index != 1 || shutdown_requested {
                break;
            }
        }
    }
    unsafe {
        sync();
        reboot(if env::var_os("LKSYSTEM_REBOOT").is_some() {
            RB_AUTOBOOT
        } else {
            RB_POWER_OFF
        });
    }
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
