use colored::*;
use nix::mount::{mount, umount, MsFlags};
use std::ffi::CString;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

type InitResult<T> = Result<T, Box<dyn std::error::Error>>;

fn log( message: &str ) { println!("{} {}", "[ i ]".bright_cyan(), message); }
fn success( message: &str ) { println!("{} {}", "[ ✓ ]".bright_green(), message.bright_green()); }
fn warning( message: &str ) { println!("{} {}", "[ ! ]".bright_yellow(), message.bright_yellow()); }
fn error( message: &str ) { println!("{} {}", "[ ✗ ]".bright_red(), message.bright_red()); }

fn main() -> InitResult<()> {
    let boot_from_iso = std::env::var("LKSYSTEM_INIT_ISO")
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    println!("");
    println!("{}", "             ::: [ WELCOME TO LISKA LINUX ] :::".bright_cyan().bold());
    println!("");
    log("Mounting pseudo filesystems....");
    mount_pseudo_fs()?;
    success("Pseudo filesystems mounted successfully!");
    if boot_from_iso {
        log("Scanning block devices for Liska Linux ISO....");
        let bootmnt = "/run/liska/bootmnt";
        fs::create_dir_all(bootmnt).ok();
        fs::create_dir_all("/src_sfs").ok();
        fs::create_dir_all("/cow").ok();
        fs::create_dir_all("/new_root").ok();
        let mut found = false;
        for _ in 0..15 {
            let _ = Command::new("/bin/mdev").arg("-s").status();
            if let Ok(entries) = fs::read_dir("/dev") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = path.to_string_lossy();
                    if name.starts_with("/dev/sd") || name.starts_with("/dev/nvme") || name.starts_with("/dev/vd") || name.starts_with("/dev/sr") {
                        if mount(Some(path.as_path()), Path::new(bootmnt), None::<&str>, MsFlags::MS_RDONLY, None::<&str>).is_ok() {
                            if Path::new(&format!("{}/liskafs.sfs", bootmnt)).exists() {
                                found = true;
                                success(&format!("Found liskafs.sfs on {}!", name.bright_cyan()));
                                break;
                            }
                            let _ = umount(bootmnt);
                        }
                    }
                }
            }
            if found { break; }
            thread::sleep(Duration::from_millis(300));
        }
        if !found {
            error("CRITICAL: could not find liskafs.sfs!");
            warning("Initializing bash shell for emergency....");
            let _ = Command::new("/bin/sh").status();
            return Ok(());
        }
        log("Mounting squashfs and setting up overlayfs....");
        mount(Some(format!("{}/liskafs.sfs", bootmnt).as_str()), "/src_sfs", Some("squashfs"), MsFlags::MS_RDONLY, None::<&str>)?;
        mount(Some("tmpfs"), "/cow", Some("tmpfs"), MsFlags::empty(), None::<&str>)?;
        fs::create_dir_all("/cow/upper").ok();
        fs::create_dir_all("/cow/work").ok();
        mount(
            Some("overlay"),
            "/new_root",
            Some("overlay"),
            MsFlags::empty(),
            Some("lowerdir=/src_sfs,upperdir=/cow/upper,workdir=/cow/work"),
        )?;
        success("Squashfs and overlayfs setup completed successfully!");
    } else {
        log("Loading storage and filesystem kernel modules....");
        load_essential_modules();
        log("Resolving root partition from cmdline....");
        let root_param = get_cmdline_param("root=");
        let real_dev = resolve_device(&root_param);
        log(&format!("Mounting root filesystem ({} -> {})....", real_dev.cyan(), "/new_root".cyan()));
        fs::create_dir_all("/new_root").ok();
        let mut mounted = false;
        for _ in 0..10 {
            let _ = Command::new("/bin/mdev").arg("-s").status();
            if mount(Some(real_dev.as_str()), "/new_root", None::<&str>, MsFlags::MS_RELATIME, None::<&str>).is_ok() 
               || mount(Some(real_dev.as_str()), "/new_root", None::<&str>, MsFlags::MS_RELATIME, Some("subvol=@")).is_ok() {
                success(&format!("Mounted root filesystem {} to {}!", real_dev.bright_cyan(), "/new_root".bright_cyan()));
                mounted = true;
                break;
            }
            thread::sleep(Duration::from_millis(300));
        }
        if !mounted {
            error(&format!("CRITICAL: could not mount root filesystem {}!", real_dev.cyan()));
            warning("Initializing bash shell for emergency....");
            let _ = Command::new("/bin/sh").status();
            return Ok(());
        }
    }
    log(&format!("Moving virtual mounts into {}....", "/new_root".cyan()));
    move_virtual_mounts("/new_root")?;
    log("Searching lksystem in new root....");
    let (init_path, init_args) = find_init_program("/new_root");
    success(&format!("Found {} in {}! Starting lksystem as PID 1.", init_path.bright_cyan(), "/new_root".bright_cyan()));
    switch_root("/new_root", &init_path, &init_args)?;
    Ok(())
}

fn mount_pseudo_fs() -> InitResult<()> {
    let _ = fs::create_dir_all("/proc");
    let _ = fs::create_dir_all("/sys");
    let _ = fs::create_dir_all("/dev");
    let _ = fs::create_dir_all("/run");
    let _ = mount(Some("proc"), "/proc", Some("proc"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("sysfs"), "/sys", Some("sysfs"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("devtmpfs"), "/dev", Some("devtmpfs"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("tmpfs"), "/run", Some("tmpfs"), MsFlags::empty(), None::<&str>);
    Ok(())
}

fn load_essential_modules() {
    let modules = [
        "ahci", "ata_piix", "libata", "sd_mod", "scsi_mod", 
        "virtio_blk", "virtio_pci", "nvme", "ext4", "btrfs", "xfs", "f2fs", "vfat", "overlay"
    ];
    for mod_name in modules {
        let _ = Command::new("/bin/modprobe").arg(mod_name).status();
    }
}

fn get_cmdline_param(param: &str) -> String {
    if let Ok(cmdline) = fs::read_to_string("/proc/cmdline") {
        for arg in cmdline.split_whitespace() {
            if arg.starts_with(param) {
                return arg.trim_start_matches(param).to_string();
            }
        }
    }
    String::new()
}

fn resolve_device(target: &str) -> String {
    if target.is_empty() { return String::new(); }
    if target.starts_with("UUID=") || target.starts_with("LABEL=") || target.starts_with("PARTUUID=") {
        if let Ok(output) = Command::new("blkid").output() {
            let out_str = String::from_utf8_lossy(&output.stdout);
            for line in out_str.lines() {
                if line.contains(target) {
                    if let Some(dev) = line.split(':').next() {
                        return dev.trim().to_string();
                    }
                }
            }
        }
    }
    target.to_string()
}

fn move_virtual_mounts(sysroot: &str) -> InitResult<()> {
    for dir in &["dev", "proc", "sys", "run"] {
        let old_path = format!("/{}", dir);
        let new_path = format!("{}/{}", sysroot, dir);
        let _ = fs::create_dir_all(&new_path);
        let _ = mount(Some(old_path.as_str()), new_path.as_str(), None::<&str>, MsFlags::MS_MOVE, None::<&str>);
    }
    Ok(())
}

fn find_init_program(sysroot: &str) -> (String, Vec<String>) {
    let candidates = ["/bin/lksystem", "/usr/bin/lksystem"];
    for cand in candidates {
        let candidate_path = format!("{}{}", sysroot, cand);
        if Path::new(&candidate_path).exists() {
            let config_dir_path = format!("{}{}", sysroot, "/etc/lksystem");
            let config_dir = Path::new(&config_dir_path);
            let args = if config_dir.exists() {
                vec![cand.to_string(), "--conf".to_string(), "/etc/lksystem".to_string()]
            } else {
                vec![cand.to_string()]
            };
            return (cand.to_string(), args);
        }
    }
    let fallback = "/sbin/init";
    if Path::new(&format!("{}{}", sysroot, fallback)).exists() {
        return (fallback.to_string(), vec![fallback.to_string()]);
    }
    let fallback2 = "/usr/lib/systemd/systemd";
    if Path::new(&format!("{}{}", sysroot, fallback2)).exists() {
        return (fallback2.to_string(), vec![fallback2.to_string()]);
    }
    ("/sbin/init".to_string(), vec!["/sbin/init".to_string()])
}

fn switch_root(sysroot: &str, init_path: &str, init_args: &[String]) -> InitResult<()> {
    std::env::set_current_dir(sysroot)?;
    nix::unistd::chroot(".")?;
    std::env::set_current_dir("/")?;
    let mut exec_args = vec![CString::new(init_path)?];
    for arg in init_args {
        exec_args.push(CString::new(arg.as_str())?);
    }
    match nix::unistd::execv(&exec_args[0], &exec_args) {
        Ok(_) => unreachable!("Execv should not return"),
        Err(err) => Err(format!(
            "CRITICAL: could not switch root to lksystem at {}: {}",
            sysroot, err
        )
        .into()),
    }
}