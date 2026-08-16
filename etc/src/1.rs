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
    // stage1 is the very first thing lksystem runs as PID 1. Nothing has
    // set PATH by this point, so a bare "mount" lookup fails with ENOENT
    // unless we provide one ourselves (same fix as mount_fstab() in 2.rs).
    let status = Command::new("mount")
        .args(["-t", filesystem, source, target])
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Could not mount {target}")))
    }
}

fn main() -> io::Result<()> {
    ui::log("Mounting proc, sys, and dev....");
    fs::create_dir_all(linux::CONFIG_DIR)?;
    fs::create_dir_all(linux::SERVICE_DIR)?;
    fs::set_permissions(linux::CONFIG_DIR, fs::Permissions::from_mode(0o755))?;
    for (source, target, filesystem) in [
        ("proc", "/proc", "proc"),
        ("sysfs", "/sys", "sysfs"),
        ("devtmpfs", "/dev", "devtmpfs"),
        ("devpts", "/dev/pts", "devpts"),
    ] {
        if let Err(error) = mount_if_needed(source, target, filesystem) {
            ui::warning(format!("{error}! Skipping...."));
        }
    }
    ui::success("Mounting process completed!");
    Ok(())
}
