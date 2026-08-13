use lksystem_ui::{linux, ui};
use std::env;
use std::io;
use std::path::Path;
use std::process::Command;

const UDEVD_CANDIDATES: [&str; 4] = [
    "/usr/lib/udev/udevd",
    "/lib/udev/udevd",
    "/usr/sbin/udevd",
    "/sbin/udevd",
];

fn start_udev() {
    ui::log("Initializing udev device manager....");
    let Some(udevd) = UDEVD_CANDIDATES
        .iter()
        .find(|candidate| Path::new(candidate).is_file())
    else {
        ui::warning("No udevd binary found! Skipping device manager startup....");
        return;
    };
    ui::log(format!("Starting udev device manager ({udevd})...."));
    match Command::new(udevd).arg("--daemon").status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            ui::warning(format!("{udevd} exited with status {status}! Skipping...."));
            return;
        }
        Err(error) => {
            ui::warning(format!("Cannot start {udevd}! Err: {error}. Skipping...."));
            return;
        }
    }
    for (args, description) in [
        (["trigger", "--type=devices", "--action=add"], "trigger"),
        (["settle", "--timeout=30", ""], "settle"),
    ] {
        let args: Vec<&str> = args.into_iter().filter(|arg| !arg.is_empty()).collect();
        match Command::new("udevadm").args(&args).status() {
            Ok(status) if status.success() => {}
            Ok(status) => ui::warning(format!("Udevadm {description} exited with status {status}!")),
            Err(error) => ui::warning(format!("Cannot run udevadm {description}! Err: {error}. Skipping....")),
        }
    }
    ui::success("Udev device manager has been started and settled!");
}

fn mount_fstab() {
    if !Path::new("/etc/fstab").is_file() {
        ui::warning("Fstab not found! Skipping....");
        return;
    }
    ui::log("Mounting filesystems from fstab....");
    match Command::new("mount").arg("-a").status() {
        Ok(status) if status.success() => ui::success("Filesystems from fstab has been mounted!"),
        Ok(status) => ui::warning(format!(
            "Mount -a exited with status {status}! Some filesystems in fstab may be missing or not get configured properly."
        )),
        Err(error) => ui::warning(format!("Cannot run mount -a! Err: {error}. Skipping....")),
    }
}

fn main() -> io::Result<()> {
    let service_dir =
        env::var("LKSYSTEM_SERVICE_DIR").unwrap_or_else(|_| linux::SERVICE_DIR.to_owned());
    let lksysdir = env::var("LKSYSTEM_lksysDIR").unwrap_or_else(|_| "lksysdir".to_owned());
    if env::var_os("LKSYSTEM_SKIP_UDEV").is_none() {
        start_udev();
    } else {
        ui::log("Skipping udev startup....");
    }
    if env::var_os("LKSYSTEM_SKIP_FSTAB").is_none() {
        mount_fstab();
    } else {
        ui::log("Skipping fstab mounts....");
    }
    match linux::activate_virtual_terminal(linux::DEFAULT_TTY) {
        Ok(true) => ui::success("Default login console has been switched to tty1!"),
        Ok(false) => ui::log("No virtual console available! Keeping the current console...."),
        Err(error) => ui::warning(format!(
            "Cannot switch default login console to tty1! Err: {error}."
        )),
    }
    ui::log(format!("Starting lksysdir for {service_dir}...."));
    let mut command = Command::new(lksysdir);
    command.args(["-P", &service_dir]).env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    ui::success("All lksystem process completed!");
    ui::log("Handing off to lksysdir....");
    Err(std::os::unix::process::CommandExt::exec(&mut command))
}
