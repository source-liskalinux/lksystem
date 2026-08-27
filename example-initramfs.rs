use std::env;
use std::error::Error;
use std::ffi::CString;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::os::raw::{c_char, c_int, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

type InitResult<T> = Result<T, Box<dyn Error>>;

const NEW_ROOT: &str = "/new_root";
const BOOT_MOUNT: &str = "/run/liska/bootmnt";
const SOURCE_SQUASHFS: &str = "/src_sfs";
const COW: &str = "/cow";
const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

const MS_RDONLY: c_ulong = 1;
const MS_MOVE: c_ulong = 8192;
const MS_RELATIME: c_ulong = 1 << 21;

unsafe extern "C" {
    fn mount(
        source: *const c_char,
        target: *const c_char,
        filesystemtype: *const c_char,
        mountflags: c_ulong,
        data: *const c_void,
    ) -> c_int;
    fn umount(target: *const c_char) -> c_int;
    fn chroot(path: *const c_char) -> c_int;
    fn execv(path: *const c_char, argv: *const *const c_char) -> c_int;
    fn sync();
}

fn main() {
    if let Err(err) = run() {
        error(&format!("CRITICAL: {err}"));
        warning("Initializing emergency shell....");
        emergency_shell();
    }
}

fn run() -> InitResult<()> {
    let boot_from_iso = env_flag("LKSYSTEM_INIT_ISO");
    log("Mounting pseudo filesystems....");
    mount_pseudo_filesystems();
    success("Pseudo filesystems has been mounted!");
    if boot_from_iso {
        mount_iso_root()?;
    } else {
        mount_real_root()?;
    }
    log(&format!("Moving virtual mounts into {NEW_ROOT}...."));
    move_virtual_mounts(NEW_ROOT)?;
    log("Searching lksystem in new root....");
    let init = find_init_program(NEW_ROOT).ok_or("No lksystem or fallback init found!")?;
    success(&format!("Starting {init} as PID 1."));
    switch_root(NEW_ROOT, &init)?;
    Ok(())
}

fn log(message: &str) {
    emit(Some("i"), "\x1b[1;36m", message, false);
}

fn success(message: &str) {
    emit(Some("+"), "\x1b[1;32m", message, true);
}

fn warning(message: &str) {
    emit(Some("!"), "\x1b[1;33m", message, true);
}

fn error(message: &str) {
    emit(Some("x"), "\x1b[1;31m", message, true);
}

fn line(message: &str) {
    emit(None, "\x1b[1;33m", message, true);
}
    
fn emit(prefix: Option<&str>, color: &str, message: &str, color_message: bool) {
    let mut stderr = io::stderr().lock();
    if stderr.is_terminal() {
        let prefix_str = match prefix {
            Some(p) => format!("[{p}] "),
            None => String::new(),
        };
        if color_message {
            let _ = writeln!(stderr, "{color}{prefix_str}{message}\x1b[0m");
        } else {
            let _ = writeln!(stderr, "{color}{prefix_str}\x1b[0m{message}");
        }
    } else {
        let _ = writeln!(stderr, "{message}");
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn mount_iso_root() -> InitResult<()> {
    log("Loading storage and ISO kernel modules....");
    load_essential_modules();
    log("Scanning block devices for Liska Linux ISO....");
    create_dirs([BOOT_MOUNT, SOURCE_SQUASHFS, COW, NEW_ROOT]);
    let iso_device = find_iso_device()?;
    success(&format!("Found liskafs.sfs on {iso_device}!"));
    log("Mounting squashfs and setting up overlayfs....");
    let sfs_path = format!("{BOOT_MOUNT}/liskafs.sfs");
    let loop_dev = setup_loop_device(&sfs_path)?;
    mount_fs(
        Some(&loop_dev),
        SOURCE_SQUASHFS,
        Some("squashfs"),
        MS_RDONLY,
        None,
    )?;
    mount_fs(Some("tmpfs"), COW, Some("tmpfs"), 0, None)?;
    let upper_dir = format!("{COW}/upper");
    let work_dir = format!("{COW}/work");
    create_dirs([&upper_dir, &work_dir]);
    let overlay_opts = format!("lowerdir={SOURCE_SQUASHFS},upperdir={upper_dir},workdir={work_dir}");
    mount_fs(
        Some("overlay"),
        NEW_ROOT,
        Some("overlay"),
        0,
        Some(&overlay_opts),
    )?;
    success("Squashfs and overlayfs are ready!");
    Ok(())
}

fn setup_loop_device(file_path: &str) -> InitResult<String> {
    run_optional("/bin/modprobe", &["loop"]);
    run_optional("/bin/mdev", &["-s"]);
    create_dirs(["/dev"]);
    if !Path::new("/dev/loop0").exists() {
        let _ = Command::new("/bin/mknod")
            .args(["/dev/loop0", "b", "7", "0"])
            .env("PATH", PATH_ENV)
            .status();
    }
    let status = Command::new("losetup")
        .args(["/dev/loop0", file_path])
        .env("PATH", PATH_ENV)
        .status();
    if let Ok(st) = status {
        if st.success() {
            return Ok("/dev/loop0".to_string());
        }
    }
    let status_auto = Command::new("losetup")
        .args(["-f", file_path])
        .env("PATH", PATH_ENV)
        .status();
    if status_auto.is_ok() && status_auto.unwrap().success() {
        if let Ok(output) = Command::new("losetup")
            .args(["-j", file_path])
            .env("PATH", PATH_ENV)
            .output()
        {
            let out_str = String::from_utf8_lossy(&output.stdout);
            if let Some(dev) = out_str.split(':').next() {
                if !dev.trim().is_empty() {
                    return Ok(dev.trim().to_string());
                }
            }
        }
    }
    Ok("/dev/loop0".to_string())
}

fn find_iso_device() -> InitResult<String> {
    let fs_types = [Some("iso9660"), Some("vfat"), Some("ext4"), Some("udf"), None];
    for _ in 0..20 {
        run_optional("/bin/mdev", &["-s"]);
        if let Ok(devices) = candidate_block_devices() {
            for device in devices {
                let device_name = device.display().to_string();
                for fs_type in &fs_types {
                    if mount_fs(
                        Some(&device_name),
                        BOOT_MOUNT,
                        *fs_type,
                        MS_RDONLY | MS_RELATIME,
                        None,
                    )
                    .is_ok()
                    {
                        if Path::new(&format!("{BOOT_MOUNT}/liskafs.sfs")).exists() {
                            return Ok(device_name);
                        }
                        let _ = umount_target(BOOT_MOUNT);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(300));
    }
    Err("could not find liskafs.sfs!".into())
}

fn candidate_block_devices() -> io::Result<Vec<PathBuf>> {
    let mut devices = Vec::new();
    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("sd")
                || name_str.starts_with("vd")
                || name_str.starts_with("nvme")
                || name_str.starts_with("sr")
                || name_str.starts_with("hd")
                || name_str.starts_with("mmcblk")
                || name_str.starts_with("loop")
            {
                devices.push(entry.path());
            }
        }
    }
    devices.sort();
    Ok(devices)
}

fn mount_real_root() -> InitResult<()> {
    log("Loading storage and filesystem kernel modules....");
    load_essential_modules();
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let root = cmdline_value(&cmdline, "root=").ok_or("missing root= kernel parameter")?;
    let root_filesystem = cmdline_value(&cmdline, "rootfstype=");
    let root_flags = cmdline_value(&cmdline, "rootflags=");
    log(&format!("Resolving target root device: {root}"));
    fs::create_dir_all(NEW_ROOT)?;
    for attempt in 1..=15 {
        run_optional("/bin/mdev", &["-s"]);
        let root_device = resolve_device(&root);
        if Path::new(&root_device).exists() {
            log(&format!("Attempting to mount {root_device} on {NEW_ROOT} ({attempt} attempt)...."));
            if mount_root_robust(
                &root_device,
                root_filesystem.as_deref(),
                root_flags.as_deref(),
            ) {
                success(&format!("Root filesystem {root_device} has been mounted to {NEW_ROOT}!"));
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(300));
    }
    Err(format!("could not mount {root} filesystem!").into())
}

fn mount_root_robust(device: &str, filesystem: Option<&str>, flags: Option<&str>) -> bool {
    if let Some(fs) = filesystem {
        if mount_fs(Some(device), NEW_ROOT, Some(fs), MS_RELATIME, flags).is_ok() {
            return true;
        }
    }
    let supported_fs = ["btrfs", "ext4", "xfs", "f2fs", "vfat", "ntfs3", "ext3", "ext2"];
    for fs in supported_fs {
        if mount_fs(Some(device), NEW_ROOT, Some(fs), MS_RELATIME, flags).is_ok() {
            return true;
        }
        if fs == "btrfs" && flags.is_none() {
            if mount_fs(
                Some(device),
                NEW_ROOT,
                Some("btrfs"),
                MS_RELATIME,
                Some("subvol=@"),
            )
            .is_ok() {
                return true;
            }
        }
    }
    mount_fs(Some(device), NEW_ROOT, None, MS_RELATIME, flags).is_ok()
}

fn load_essential_modules() {
    let modules = [
        // Controller and Storage Drivers
        "loop",
        "ahci",
        "ata_piix",
        "libata",
        "sd_mod",
        "sr_mod",
        "cdrom",
        "scsi_mod",
        "virtio_blk",
        "virtio_pci",
        "nvme",
        "mmc_block",
        "sdhci",
        // Filesystem Drivers
        "ext4",
        "btrfs",
        "xfs",
        "f2fs",
        "vfat",
        "fat",
        "iso9660",
        "isofs",
        "udf",
        "ntfs3",
        "squashfs",
        "overlay",
    ];
    for module in modules {
        run_optional("/bin/modprobe", &[module]);
    }
}

fn cmdline_value(cmdline: &str, key: &str) -> Option<String> {
    cmdline
        .split_whitespace()
        .find_map(|arg| arg.strip_prefix(key).map(ToOwned::to_owned))
        .filter(|value| !value.is_empty())
}

fn resolve_device(target: &str) -> String {
    if target.starts_with("/dev/") && Path::new(target).exists() {
        return target.to_owned();
    }
    run_optional("/bin/mdev", &["-s"]);
    let (key, val) = if let Some(v) = target.strip_prefix("UUID=") {
        ("UUID", v)
    } else if let Some(v) = target.strip_prefix("LABEL=") {
        ("LABEL", v)
    } else if let Some(v) = target.strip_prefix("PARTUUID=") {
        ("PARTUUID", v)
    } else if let Some(v) = target.strip_prefix("PARTLABEL=") {
        ("PARTLABEL", v)
    } else {
        ("", "")
    };
    if !key.is_empty() && !val.is_empty() {
        if let Some(dev) = find_device_by_tag(key, val) {
            return dev;
        }
    }
    target.to_owned()
}

fn find_device_by_tag(key: &str, val: &str) -> Option<String> {
    if key == "UUID" || key == "LABEL" {
        let flag = if key == "UUID" { "-U" } else { "-L" };
        if let Ok(output) = Command::new("blkid")
            .args([flag, val])
            .env("PATH", PATH_ENV)
            .output()
        {
            if output.status.success() {
                let dev = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !dev.is_empty() && Path::new(&dev).exists() {
                    return Some(dev);
                }
            }
        }
    }
    if let Ok(output) = Command::new("blkid").env("PATH", PATH_ENV).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let Some((device, rest)) = line.split_once(':') else {
                continue;
            };
            let device = device.trim();
            if device.is_empty() {
                continue;
            }
            for (attr_key, attr_val) in parse_blkid_attrs(rest.trim()) {
                if attr_key.eq_ignore_ascii_case(key) && attr_val == val && Path::new(device).exists() {
                    return Some(device.to_string());
                }
            }
        }
    }
    if let Ok(devices) = candidate_block_devices() {
        for dev in devices {
            let dev_str = dev.display().to_string();
            if let Ok(output) = Command::new("blkid")
                .args(["-s", key, "-o", "value", &dev_str])
                .env("PATH", PATH_ENV)
                .output()
            {
                if output.status.success() {
                    let attr_val = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if attr_val == val {
                        return Some(dev_str);
                    }
                }
            }
        }
    }
    None
}

fn parse_blkid_attrs(rest: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut remainder = rest;
    while let Some(eq_idx) = remainder.find('=') {
        let key = remainder[..eq_idx].trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            break;
        }
        let after_eq = &remainder[eq_idx + 1..];
        let (value, tail) = if let Some(stripped) = after_eq.strip_prefix('"') {
            match stripped.find('"') {
                Some(end) => (&stripped[..end], &stripped[end + 1..]),
                None => (stripped, ""),
            }
        } else {
            match after_eq.find(' ') {
                Some(end) => (&after_eq[..end], &after_eq[end..]),
                None => (after_eq, ""),
            }
        };
        attrs.push((key.to_string(), value.to_string()));
        remainder = tail.trim_start();
        if remainder.is_empty() {
            break;
        }
    }
    attrs
}

fn move_virtual_mounts(sysroot: &str) -> InitResult<()> {
    for dir in ["dev", "proc", "sys", "run"] {
        let old_path = format!("/{dir}");
        let new_path = format!("{sysroot}/{dir}");
        fs::create_dir_all(&new_path)?;
        mount_fs(Some(&old_path), &new_path, None, MS_MOVE, None)?;
    }
    Ok(())
}

// Here you can add your init or system manager that you'll use.
// You can add runit, systemd, or other system manager that can
// run on Linux properly.
fn find_init_program(sysroot: &str) -> Option<String> {
    [
        "/usr/sbin/lksystem",
        "/sbin/lksystem",
        "/usr/bin/lksystem",
        "/bin/lksystem",
        "/sbin/init",
        "/usr/sbin/init",
        "/bin/sh",
    ]
    .into_iter()
    .find(|candidate| Path::new(&format!("{sysroot}{candidate}")).exists())
    .map(ToOwned::to_owned)
}

fn switch_root(sysroot: &str, init_path: &str) -> InitResult<()> {
    env::set_current_dir(sysroot)?;
    let current_dir = CString::new(".")?;
    let root_path = CString::new("/")?;
    unsafe {
        if mount(current_dir.as_ptr(), root_path.as_ptr(), std::ptr::null(), MS_MOVE, std::ptr::null()) != 0 {
            warning("MS_MOVE failed! Attempting fallback chroot....");
        }
    }
    chroot_to(".")?;
    env::set_current_dir("/")?;
    // execv() has no envp of its own, it just inherits whatever's already
    // in this process's environment. Nothing earlier in this file ever sets
    // PATH on the normal boot path (only emergency_shell() did), so make
    // sure the new PID 1 (lksystem or whatever init that has been found) 
    // actually gets a usable PATH instead of starting with none at all.
    unsafe {
        env::set_var("PATH", PATH_ENV);
    }
    if let Err(e) = exec_program(init_path) {
        error(&format!("CRITICAL: Failed to exec {init_path}: {e}"));
        error("CRITICAL: Failed to replace PID 1 process! Emergency shell will be initialize to prevent kernel panic!");
        warning("Initializing emergency shell....");
        emergency_shell();
    }
    unreachable!("Execv returned success");
}

fn mount_pseudo_filesystems() {
    create_dirs(["/proc", "/sys", "/dev", "/run"]);
    mount_if_needed("proc", "/proc", "proc");
    mount_if_needed("sysfs", "/sys", "sysfs");
    mount_if_needed("devtmpfs", "/dev", "devtmpfs");
    mount_if_needed("tmpfs", "/run", "tmpfs");
    create_dirs(["/dev/pts"]);
    mount_if_needed("devpts", "/dev/pts", "devpts");
}

fn mount_if_needed(source: &str, target: &str, filesystem: &str) {
    if is_mount_point(target) {
        return;
    }
    if let Err(error) = mount_fs(Some(source), target, Some(filesystem), 0, None) {
        warning(&format!("Could not mount {target}! Err: {error}."));
    }
}

fn is_mount_point(target: &str) -> bool {
    let path = Path::new(target);
    if !path.exists() {
        return false;
    }
    if let Ok(mountinfo) = fs::read_to_string("/proc/self/mountinfo") {
        mountinfo
            .lines()
            .any(|line| line.split(' ').nth(4) == Some(target))
    } else {
        false
    }
}

fn create_dirs<const N: usize>(paths: [&str; N]) {
    for path in paths {
        let _ = fs::create_dir_all(path);
    }
}

fn run_optional(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .env("PATH", PATH_ENV)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn new_root_is_mounted() -> bool {
    is_mount_point(NEW_ROOT)
}

fn do_reboot() -> ! {
    warning("Rebooting....");
    unsafe { sync() };
    thread::sleep(Duration::from_secs(1));
    const REBOOT_CANDIDATES: [(&str, Option<&str>); 4] = [
        ("reboot", None),
        ("/bin/reboot", None),
        ("/usr/bin/reboot", None),
        ("/bin/busybox", Some("reboot")),
    ];
    for (program, arg) in REBOOT_CANDIDATES {
        let mut command = Command::new(program);
        if let Some(arg) = arg {
            command.arg(arg);
        }
        command.env("PATH", PATH_ENV);
        match command.status() {
            Ok(status) if status.success() => {
                warning(&format!("Handed off to {program}. Waiting for kernel to restart...."));
                thread::sleep(Duration::from_secs(3));
            }
            Ok(_) | Err(_) => continue,
        }
    }
    error("CRITICAL: Reboot command unavailable or ineffective! Halting the system to prevent kernel panic!");
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn try_resume_boot() {
    if new_root_is_mounted() {
        warning("Detected mounted '/new_root'! Attempting to resume boot process....");
        for dir in ["dev", "proc", "sys", "run"] {
            let old = format!("/{dir}");
            let new = format!("{NEW_ROOT}/{dir}");
            let _ = fs::create_dir_all(&new);
            let _ = mount_fs(Some(&old), &new, None, MS_MOVE, None);
        }
        match find_init_program(NEW_ROOT) {
            Some(init) => {
                success(&format!("Starting {init} as PID 1...."));
                if let Err(e) = switch_root(NEW_ROOT, &init) {
                    error(&format!("CRITICAL: switch_root failed: {e}!"));
                    do_reboot();
                }
            }
            None => {
                error("CRITICAL: No init found in '/new_root' even after manual fix!");
                do_reboot();
            }
        }
    } else {
        error("CRITICAL: '/new_root' is not mounted!");
        do_reboot();
    }
}

fn emergency_shell() -> ! {
    unsafe {
        env::set_var("PATH", PATH_ENV);
    }
    const SHELL_CANDIDATES: [(&str, Option<&str>); 6] = [
        ("/bin/cttyhack", Some("/bin/sh")),
        ("/bin/cttyhack", Some("/bin/bash")),
        ("/bin/sh", Some("-i")),
        ("/bin/bash", Some("-i")),
        ("/bin/busybox", Some("sh")),
        ("/usr/bin/busybox", Some("sh")),
    ];
    if !SHELL_CANDIDATES
        .iter()
        .any(|(program, _)| Path::new(program).exists())
    {
        error("CRITICAL: No binary shell found in initramfs!");
        error("CRITICAL: Cannot start emergency shell! Rebooting in 10 seconds....");
        thread::sleep(Duration::from_secs(10));
        do_reboot();
    }
    warning("You are now on emergency bash shell!");
    line("");
    line("------------------------------------------------------------------------------------------------------------------");
    line("");
    line("> TIPS for debugging:");
    line("  - WARNING: Before retry or reboot, made sure you already FIX THE PROBLEM!");
    line("  - WARNING: If you didn't fix the problem before retry or reboot, init will fall to emergency shell again!");
    line("  - Type 'exit' or Ctrl + D when done:");
    line("     * If /new_root is mounted -> init will resume boot automatically.");
    line("     * If /new_root is NOT yet mounted -> system will reboot for a clean retry.");
    line("  - After reboot, edit '/etc/lkinit.d/init.rs' file before running lkinit again.");
    line("");
    line("------------------------------------------------------------------------------------------------------------------");
    line("");
    line("  Goodluck. I know you can do it! ;>");
    line("");
    thread::sleep(Duration::from_secs(1));
    for (program, arg) in SHELL_CANDIDATES {
        if !Path::new(program).exists() {
            continue;
        }
        let mut command = Command::new(program);
        if let Some(arg) = arg {
            command.arg(arg);
        }
        command.env("PATH", PATH_ENV).env("TERM", "linux");
        match command.status() {
            Ok(_) => {
                try_resume_boot();
            }
            Err(e) => {
                warning(&format!("Cannot start {program}! Err: {e}. Trying next...."));
                continue;
            }
        }
    }
    error("CRITICAL: Every shell candidate failed to start! Rebooting in 10 seconds....");
    thread::sleep(Duration::from_secs(10));
    do_reboot();
}

fn mount_fs(
    source: Option<&str>,
    target: &str,
    filesystem: Option<&str>,
    flags: c_ulong,
    data: Option<&str>,
) -> io::Result<()> {
    let source = optional_cstring(source)?;
    let target = CString::new(target)?;
    let filesystem = optional_cstring(filesystem)?;
    let data = optional_cstring(data)?;
    let source_ptr = source.as_ref().map_or(std::ptr::null(), |value| value.as_ptr());
    let filesystem_ptr = filesystem
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());
    let data_ptr = data
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr() as *const c_void);
    let result = unsafe {
        mount(
            source_ptr,
            target.as_ptr(),
            filesystem_ptr,
            flags,
            data_ptr,
        )
    };
    syscall_result(result)
}

fn umount_target(target: &str) -> io::Result<()> {
    let target = CString::new(target)?;
    syscall_result(unsafe { umount(target.as_ptr()) })
}

fn chroot_to(path: &str) -> io::Result<()> {
    let path = CString::new(path)?;
    syscall_result(unsafe { chroot(path.as_ptr()) })
}

fn exec_program(program: &str) -> io::Result<()> {
    let prog_cstr = CString::new(program)?;
    let argv: [*const c_char; 2] = [prog_cstr.as_ptr(), std::ptr::null()];
    unsafe {
        execv(prog_cstr.as_ptr(), argv.as_ptr());
    }
    Err(io::Error::last_os_error())
}

fn syscall_result(result: c_int) -> io::Result<()> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn optional_cstring(value: Option<&str>) -> io::Result<Option<CString>> {
    value
        .map(CString::new)
        .transpose()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}
