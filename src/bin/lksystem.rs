use lksystem::core::{install_signal_handlers, take_terminate, REBOOT_CMD_FILE, SIGTERM};
use lksystem::ui;
use std::env;
use std::fs;
use std::io;
use std::io::Write;
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
const RB_HALT_SYSTEM: i32 = 0xCDEF_0123_u32 as i32;
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

fn clear_console() {
    if let Ok(mut console) = fs::OpenOptions::new().write(true).open("/dev/console") {
        let _ = console.write_all(b"\x1b[H\x1b[2J\x1b[3J");
    }
}

fn main() -> io::Result<()> {
    if unsafe { getpid() } != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "lksystem must run as PID 1!",
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
        // lksysctl (or anything else) writes the desired action here right
        // before sending SIGTERM. A plain `kill -TERM 1` with no file
        // present falls back to the old LKSYSTEM_REBOOT env-var behavior,
        // so nothing that relied on that directly is broken.
        let action = fs::read_to_string(REBOOT_CMD_FILE)
            .ok()
            .map(|contents| contents.trim().to_owned())
            .unwrap_or_else(|| {
                if env::var_os("LKSYSTEM_REBOOT").is_some() {
                    "reboot".to_owned()
                } else {
                    "shutdown".to_owned()
                }
            });
        clear_console();
        ui::success(format!("System {action} signal received!"));
        ui::success("All services has been stopped!");
        ui::log(format!("System will {action} now...."));
        reboot(match action.as_str() {
            "reboot" => RB_AUTOBOOT,
            "halt" => RB_HALT_SYSTEM,
            _ => RB_POWER_OFF,
        });
    }
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
