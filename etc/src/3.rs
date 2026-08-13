use lksystem_ui::{linux, ui};
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

fn control_services(action: &str, services: &[String]) {
    if services.is_empty() {
        return;
    }
    match Command::new("lksysctl")
        .args(["-w196", action])
        .args(services)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => ui::warning(format!("Lksysctl {action} exited with status {status}!")),
        Err(error) => ui::warning(format!("Cannot run lksysctl {action}! Err: {error}.")),
    }
}

fn main() -> io::Result<()> {
    let service_dir = Path::new(linux::SERVICE_DIR);
    let mut services = Vec::new();
    if service_dir.is_dir() {
        for entry in fs::read_dir(service_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                services.push(entry.path().display().to_string());
            }
        }
    }
    ui::log("Waiting for services to stop....");
    control_services("force-stop", &services);
    control_services("exit", &services);
    ui::success("All process completed!");
    Ok(())
}
