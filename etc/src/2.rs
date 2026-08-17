use lksystem_ui::{linux, ui};
use std::env;
use std::io;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn start_udev() {
    ui::log("Initializing udev....");
    match Command::new("/usr/lib/udev/udevd").arg("--daemon").status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            ui::warning(format!("Udevd exited with status {status}! Skipping...."));
            return;
        }
        Err(error) => {
            ui::warning(format!("Cannot start udevd! Err: {error}. Skipping...."));
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
            Ok(status) => ui::warning(format!("Udevadm {description} exited with status {status}! Skipping....")),
            Err(error) => ui::warning(format!("Cannot run udevadm {description}! Err: {error}. Skipping....")),
        }
    }
    ui::success("Udev has been started and settled!");
}

fn mount_fstab() {
    if !Path::new("/etc/fstab").is_file() {
        ui::warning("Fstab file not found! Skipping....");
        return;
    }
    ui::log("Mounting filesystems from fstab....");
    // lksystem starts as PID 1 with no PATH set by anyone up the boot chain
    // (the initramfs never sets one on the normal boot path), so a bare
    // "mount" lookup fails with ENOENT here unless we provide a PATH
    // ourselves, same fix already applied below for the lksysdir handoff.
    match Command::new("mount")
        .arg("-a")
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .status()
    {
        Ok(status) if status.success() => ui::success("Filesystems from fstab has been mounted!"),
        Ok(status) => ui::warning(format!(
            "Mount exited with status {status}! Some filesystems in fstab may be missing or not get configured properly! Skipping...."
        )),
        Err(error) => ui::warning(format!("Cannot run mount! Err: {error}. Skipping....")),
    }
}

fn main() -> io::Result<()> {
    let service_dir =
        env::var("LKSYSTEM_SERVICE_DIR").unwrap_or_else(|_| linux::SERVICE_DIR.to_owned());
    let lksysdir = env::var("LKSYSTEM_lksysDIR").unwrap_or_else(|_| "lksysdir".to_owned());
    if env::var_os("LKSYSTEM_SKIP_UDEV").is_none() {
        start_udev();
    } else {
        ui::log("Skipping udev startup process....");
    }
    if env::var_os("LKSYSTEM_SKIP_FSTAB").is_none() {
        mount_fstab();
    } else {
        ui::log("Skipping fstab mount process....");
    }
    match linux::activate_virtual_terminal(linux::DEFAULT_TTY) {
        Ok(true) => ui::success("Default login console has been switched to tty1!"),
        Ok(false) => ui::log("No virtual console available! Keeping the current console...."),
        Err(error) => ui::warning(format!(
            "Cannot switch default login console to tty1! Err: {error}."
        )),
    }
    ui::success("All lksystem process completed!");
    ui::log(format!("Starting lksysdir...."));
    let mut command = Command::new(&lksysdir);
    command.args(["-P", &service_dir]).env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    ui::log("Handing off to lksysdir....");
    let err = std::os::unix::process::CommandExt::exec(&mut command);
    if err.kind() == io::ErrorKind::NotFound {
        ui::error(format!("Cannot find or exec lksysdir!"));
    } else {
        ui::error(format!("Cannot exec lksysdir! Err: {err}."));
    }
    ui::error("CRITICAL: Lksysdir could not be started! Falling back to lksystem emergency shell!");
    ui::error("Initializing emergency shell....");
    ui::warning("NOTE: No services will be supervised until this the problem is fixed and lksystem is restarted!");
    run_tty1_shell();
}

// lksystem equivalent of the initramfs emergency shell, it's only reached
// when handing off to lksysdir fails. Spawns an interactive shell directly
// on whatever console activate_virtual_terminal() switched to and respawns
// it if it exits, so there's always a way in even when the real service 
// supervisor can't start.
fn run_tty1_shell() -> ! {
    const SHELL_CANDIDATES: [(&str, Option<&str>); 4] = [
        ("/bin/cttyhack", Some("/bin/sh")),
        ("/bin/cttyhack", Some("/bin/bash")),
        ("/bin/sh", Some("-i")),
        ("/bin/bash", Some("-i")),
    ];
    ui::line("");
    ui::error("You are now on emergency bash shell! Good luck!");
    ui::line("");
    loop {
        for (program, arg) in SHELL_CANDIDATES {
            let mut command = Command::new(program);
            if let Some(arg) = arg {
                command.arg(arg);
            }
            command.env("TERM", "linux").env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            );
            match command.status() {
                // Shell exited cleanly (e.g. "exit" or Ctrl+D) - respawn it.
                Ok(status) if status.success() => break,
                Ok(status) => ui::warning(format!("{program} exited with status {status}!")),
                Err(error) => ui::warning(format!("Cannot start {program}! Err: {error}.")),
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}
