use lksystem::core::{install_signal_handlers, take_reload, take_terminate};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

unsafe extern "C" {
    fn dup2(oldfd: i32, newfd: i32) -> i32;
}

fn attach_console_stderr() {
    if let Ok(console) = fs::OpenOptions::new().write(true).open("/dev/console") {
        unsafe {
            dup2(console.as_raw_fd(), 2);
        }
    }
}

fn usage() -> ! {
    println!("");
    println!("Usage: lksysdir <-P> [dir]");
    println!("");
    std::process::exit(1);
}

fn lksys_binary() -> io::Result<PathBuf> {
    let current = env::current_exe()?;
    Ok(current
        .parent()
        .map(|directory| directory.join("lksys"))
        .unwrap_or_else(|| PathBuf::from("lksys")))
}

fn discover(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut services = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            services.push(entry.path());
        }
    }
    services.sort();
    Ok(services)
}

fn spawn_lksys(binary: &Path, service: &Path) -> io::Result<Child> {
    Command::new(binary)
        .arg(service)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn is_agetty(service: &Path) -> bool {
    service
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("agetty"))
}

fn is_dbus(service: &Path) -> bool {
    service
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "dbus")
}

// Wipes the screen and scrollback on the active console, the same way
// systemd clears the console right before it hands off to agetty. Called
// once, right as the agetty services are first released, so boot logs from
// the other services don't linger under the login prompt.
fn clear_console() {
    if let Ok(mut console) = fs::OpenOptions::new().write(true).open("/dev/console") {
        let _ = console.write_all(b"\x1b[H\x1b[2J\x1b[3J");
    }
}

fn main() -> io::Result<()> {
    attach_console_stderr();
    let mut arguments = env::args_os().skip(1);
    let process_group = match arguments.next() {
        Some(option) if option == "-P" => true,
        Some(option) => {
            let directory = PathBuf::from(option);
            if arguments.next().is_some() {
                usage();
            }
            return supervise(directory, false);
        }
        None => usage(),
    };
    let Some(directory) = arguments.next() else {
        usage()
    };
    if arguments.next().is_some() {
        usage();
    }
    supervise(PathBuf::from(directory), process_group)
}

fn supervise(directory: PathBuf, _process_group: bool) -> io::Result<()> {
    install_signal_handlers();
    let binary = lksys_binary()?;
    let mut children: HashMap<PathBuf, Child> = HashMap::new();
    let mut console_cleared = false;
    loop {
        let services = discover(&directory)?;
        // dbus gets a head start: every other non-agetty service waits until
        // dbus has been spawned at least once, so services that depend on
        // the system bus (e.g. networkmanager) don't race it on boot.
        let dbus_started = services
            .iter()
            .find(|service| is_dbus(service))
            .map_or(true, |service| children.contains_key(service.as_path()));
        let others_started = services
            .iter()
            .filter(|service| !is_agetty(service))
            .all(|service| children.contains_key(service.as_path()));
        if others_started && !console_cleared {
            clear_console();
            console_cleared = true;
        }
        for service in &services {
            if is_agetty(service) && !others_started && !children.contains_key(service) {
                continue;
            }
            if !is_agetty(service)
                && !is_dbus(service)
                && !dbus_started
                && !children.contains_key(service)
            {
                continue;
            }
            let exit_status = children
                .get_mut(service)
                .map(|child| child.try_wait())
                .transpose()?
                .flatten();
            let restart = exit_status.is_some() || !children.contains_key(service);
            if restart {
                children.insert(service.clone(), spawn_lksys(&binary, service)?);
            }
        }
        children.retain(|service, child| {
            if services.contains(service) {
                true
            } else {
                let _ = child.kill();
                false
            }
        });
        if take_reload() {
            continue;
        }
        if take_terminate() {
            for child in children.values_mut() {
                let _ = child.kill();
            }
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
}
