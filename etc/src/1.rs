use lksystem_ui::{linux, ui};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn is_mount_point(target: &str) -> bool {
    fs::read_to_string("/proc/self/mountinfo")
        .map(|mountinfo| {
            mountinfo
                .lines()
                .any(|line| line.split(' ').nth(4) == Some(target))
        })
        .unwrap_or(false)
}

fn mount_if_needed(source: &str, target: &str, filesystem: &str) -> io::Result<()> {
    if is_mount_point(target) {
        return Ok(());
    }
    fs::create_dir_all(target)?;
    let status = Command::new("mount")
        .args(["-t", filesystem, source, target])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Could not mount {target}")))
    }
}

fn main() -> io::Result<()> {
    ui::log("Entering lksystem stage 1....");
    fs::create_dir_all(linux::CONFIG_DIR)?;
    fs::create_dir_all(linux::SERVICE_DIR)?;
    fs::set_permissions(linux::CONFIG_DIR, fs::Permissions::from_mode(0o755))?;
    // A normal Linux boot needs these before D-Bus and NetworkManager.
    // Existing mounts are intentionally left untouched
    for (source, target, filesystem) in [
        ("proc", "/proc", "proc"),
        ("sysfs", "/sys", "sysfs"),
        ("devtmpfs", "/dev", "devtmpfs"),
        ("devpts", "/dev/pts", "devpts"),
    ] {
        if let Err(error) = mount_if_needed(source, target, filesystem) {
            ui::warning(format!("{error}! Continuing...."));
        }
    }
    ui::success("Lksystem stage 1 completed successfully!");
    Ok(())
}
