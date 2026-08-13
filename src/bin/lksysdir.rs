use lksystem::core::{install_signal_handlers, take_reload, take_terminate};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

fn usage() -> ! {
    eprintln!("usage: lksysdir [-P] dir");
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
    Command::new(binary).arg(service).spawn()
}

fn main() -> io::Result<()> {
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
    loop {
        let services = discover(&directory)?;
        for service in &services {
            let restart = children
                .get_mut(service)
                .map(|child| child.try_wait().map(|status| status.is_some()))
                .transpose()?
                .unwrap_or(true);
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
