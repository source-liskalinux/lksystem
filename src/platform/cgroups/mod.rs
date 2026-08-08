// This module provides methods to manage processes with cgroups. Not resource management but reliable tracking of services.
// It dynamically decides wether cgroups v1 or v2 should be used.
// The cgroup paths created by get_own_freezer return a path that is inside the cgroup that contains lksystem itself. With the naming scheme of the freezer
// cgroups we should mostly comply to the guidelines here https://www.freedesktop.org/wiki/Software/systemd/PaxControlGroups/
use std::fs;
use std::io::{Read, Write};
use crate::ui;
mod cgroup1;
mod cgroup2;

#[derive(Debug)]
pub enum CgroupError {
    IOErr(std::io::Error, String),
    NixErr(nix::Error),
    NotMounted,
}

impl std::fmt::Display for CgroupError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        let msg = match self {
            CgroupError::IOErr(e, f) => format!("io error: {}, file: {}", e, f),
            CgroupError::NixErr(e) => format!("nix error: {}", e),
            CgroupError::NotMounted => "The freezer cgroup was not mounted".into(),
        };
        fmt.write_str(format!("{}", msg).as_str())
    }
}

fn use_v2(cgroup_path: &std::path::PathBuf) -> bool {
    let freeze_file = cgroup_path.join("cgroup.freeze");
    let exists = freeze_file.exists();
    ui::log(format!("{:?} exists: {}", freeze_file, exists));
    exists
}

fn write_cgroup_value(
    cgroup_path: &std::path::PathBuf,
    file_name: &str,
    value: &str,
) -> Result<(), CgroupError> {
    let cgroup_file = cgroup_path.join(file_name);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(&cgroup_file)
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_file)))?;
    f.write_all(value.as_bytes())
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_file)))?;
    Ok(())
}

fn parse_memory_bytes(value: &str) -> Result<u64, CgroupError> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("infinity") || value.eq_ignore_ascii_case("max") {
        return Ok(u64::MAX);
    }
    let mut number = String::new();
    let mut suffix = String::new();
    for c in value.chars() {
        if c.is_ascii_digit() {
            if suffix.is_empty() {
                number.push(c);
            } else {
                return Err(CgroupError::IOErr(
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid memory size"),
                    value.to_string(),
                ));
            }
        } else if c.is_ascii_whitespace() {
            continue;
        } else {
            suffix.push(c);
        }
    }
    let bytes = number.parse::<u64>().map_err(|_| {
        CgroupError::IOErr(
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid memory size"),
            value.to_string(),
        )
    })?;
    let multiplier = match suffix.to_ascii_lowercase().as_str() {
        "" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        "t" | "tb" => 1024 * 1024 * 1024 * 1024,
        _ => {
            return Err(CgroupError::IOErr(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Unsupported memory suffix"),
                value.to_string(),
            ));
        }
    };
    Ok(bytes.saturating_mul(multiplier))
}

fn parse_percentage(value: &str) -> Result<u64, CgroupError> {
    let value = value.trim();
    if let Some(pct) = value.strip_suffix('%') {
        let percent = pct
            .trim()
            .parse::<u64>()
            .map_err(|_| CgroupError::IOErr(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid percent"),
                value.to_string(),
            ))?;
        Ok(percent)
    } else {
        value
            .parse::<u64>()
            .map_err(|_| CgroupError::IOErr(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid percent"),
                value.to_string(),
            ))
    }
}

const CGROUP2_CPU_PERIOD: u64 = 100_000;

fn set_cpu_quota_v2(cgroup_path: &std::path::PathBuf, cpu_quota: &str) -> Result<(), CgroupError> {
    let cpu_quota = cpu_quota.trim();
    if cpu_quota.eq_ignore_ascii_case("max") {
        return write_cgroup_value(cgroup_path, "cpu.max", "max");
    }
    let quota = if cpu_quota.ends_with('%') {
        let percent = parse_percentage(cpu_quota)?;
        if percent == 0 {
            return Err(CgroupError::IOErr(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "CPUQuota must be greater than 0"),
                cpu_quota.to_string(),
            ));
        }
        let period = CGROUP2_CPU_PERIOD;
        let quota = period.saturating_mul(percent).checked_div(100).ok_or_else(|| {
            CgroupError::IOErr(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid CPUQuota percent"),
                cpu_quota.to_string(),
            )
        })?;
        format!("{} {}", quota, period)
    } else {
        // treat as absolute microseconds or percentage when within 1..100
        let value = parse_percentage(cpu_quota)?;
        if value > 100 {
            format!("{} {}", value, CGROUP2_CPU_PERIOD)
        } else {
            let period = CGROUP2_CPU_PERIOD;
            let quota = period.saturating_mul(value).checked_div(100).ok_or_else(|| {
                CgroupError::IOErr(
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid CPUQuota percent"),
                    cpu_quota.to_string(),
                )
            })?;
            format!("{} {}", quota, period)
        }
    };
    write_cgroup_value(cgroup_path, "cpu.max", &quota)
}

fn set_cpu_quota_v1(cgroup_path: &std::path::PathBuf, cpu_quota: &str) -> Result<(), CgroupError> {
    let cpu_quota = cpu_quota.trim();
    if cpu_quota.eq_ignore_ascii_case("max") {
        write_cgroup_value(cgroup_path, "cpu.cfs_quota_us", "-1")
    } else {
        let quota = if cpu_quota.ends_with('%') {
            let percent = parse_percentage(cpu_quota)?;
            if percent == 0 {
                return Err(CgroupError::IOErr(
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "CPUQuota must be greater than 0"),
                    cpu_quota.to_string(),
                ));
            }
            let period = CGROUP2_CPU_PERIOD;
            period.saturating_mul(percent).checked_div(100).ok_or_else(|| {
                CgroupError::IOErr(
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid CPUQuota percent"),
                    cpu_quota.to_string(),
                )
            })?
        } else {
            parse_percentage(cpu_quota)?
        };
        write_cgroup_value(cgroup_path, "cpu.cfs_quota_us", &quota.to_string())
            .and_then(|_| write_cgroup_value(cgroup_path, "cpu.cfs_period_us", &CGROUP2_CPU_PERIOD.to_string()))
    }
}

fn set_tasks_max(cgroup_path: &std::path::PathBuf, tasks_max: &str) -> Result<(), CgroupError> {
    let value = tasks_max.trim();
    let target = if value.eq_ignore_ascii_case("infinity") || value.eq_ignore_ascii_case("max") {
        "max".to_string()
    } else {
        value.parse::<u64>()
            .map_err(|_| CgroupError::IOErr(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid tasks max"),
                value.to_string(),
            ))?
            .to_string()
    };
    if use_v2(cgroup_path) {
        write_cgroup_value(cgroup_path, "pids.max", &target)
    } else {
        match write_cgroup_value(cgroup_path, "pids.max", &target) {
            Ok(_) => Ok(()),
            Err(_) => write_cgroup_value(cgroup_path, "tasks.max", &target),
        }
    }
}

fn set_memory_max(cgroup_path: &std::path::PathBuf, memory_max: &str) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        write_cgroup_value(cgroup_path, "memory.max", memory_max.trim())
    } else {
        let bytes = parse_memory_bytes(memory_max)?;
        write_cgroup_value(cgroup_path, "memory.limit_in_bytes", &bytes.to_string())
    }
}

fn set_cpu_weight(cgroup_path: &std::path::PathBuf, cpu_weight: u64) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        write_cgroup_value(cgroup_path, "cpu.weight", &cpu_weight.to_string())
    } else {
        let weight = cpu_weight.clamp(1, 10000);
        let shares = ((weight as u64 * 1024) + 50) / 100;
        let shares = shares.clamp(2, 262144);
        write_cgroup_value(cgroup_path, "cpu.shares", &shares.to_string())
    }
}

fn set_io_weight(cgroup_path: &std::path::PathBuf, io_weight: u64) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        write_cgroup_value(cgroup_path, "io.weight", &io_weight.to_string())
    } else {
        write_cgroup_value(cgroup_path, "blkio.weight", &io_weight.to_string())
    }
}

pub fn apply_service_cgroup_settings(
    cgroup_path: &std::path::PathBuf,
    cpu_quota: &Option<String>,
    cpu_weight: &Option<u64>,
    memory_max: &Option<String>,
    tasks_max: &Option<String>,
    io_weight: &Option<u64>,
) -> Result<(), CgroupError> {
    if let Some(cpu_quota) = cpu_quota {
        if use_v2(cgroup_path) {
            set_cpu_quota_v2(cgroup_path, cpu_quota)?;
        } else {
            set_cpu_quota_v1(cgroup_path, cpu_quota)?;
        }
    }
    if let Some(cpu_weight) = cpu_weight {
        set_cpu_weight(cgroup_path, *cpu_weight)?;
    }
    if let Some(memory_max) = memory_max {
        set_memory_max(cgroup_path, memory_max)?;
    }
    if let Some(tasks_max) = tasks_max {
        set_tasks_max(cgroup_path, tasks_max)?;
    }
    if let Some(io_weight) = io_weight {
        set_io_weight(cgroup_path, *io_weight)?;
    }
    Ok(())
}

const OWN_CGROUP_NAME: &str = "lksystem_self";

// moves lksystem into own cgroup if v2 is used
//
// This is necessary because cgroupv2 discourages processes in cgroups that are not leafes
pub fn move_to_own_cgroup(base_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    ui::log(format!("Move lksystem to own manager cgroup"));
    let proc_content = match std::fs::read_to_string("/proc/self/cgroup") {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };
    let proc_content_lines = proc_content.split('\n').collect::<Vec<_>>();
    let v2path = get_own_cgroup_v2(&proc_content_lines);
    ui::log(format!("V2 path: {:?}", v2path));
    if let Some(v2path) = v2path {
        let base_path = base_path.join("unified");
        let absolute_v2path = base_path.join(v2path);
        let lksystem_subgroup = absolute_v2path.join(format!("lksystem_{}", nix::unistd::getpid()));
        let manager_cgroup = lksystem_subgroup.join(OWN_CGROUP_NAME);
        ui::log(format!("Manager path: {:?}", manager_cgroup));
        if !manager_cgroup.exists() {
            if let Err(e) = std::fs::create_dir_all(&manager_cgroup) {
                ui::log(format!("Skipping manager cgroup creation: {}", e));
                return Ok(());
            }
        }
        if let Err(e) = move_self_to_cgroup(&manager_cgroup) {
            ui::log(format!("Skipping manager cgroup move: {}", e));
        }
    }
    Ok(())
}

pub fn move_out_of_own_cgroup(base_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    let proc_content = match std::fs::read_to_string("/proc/self/cgroup") {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };
    let proc_content_lines = proc_content.split('\n').collect::<Vec<_>>();
    if let Some(v2path) = crate::platform::cgroups::get_own_cgroup_v2(&proc_content_lines) {
        let absolute_v2path = base_path.join(v2path);
        let mut parent_group = absolute_v2path.clone();
        parent_group.pop();
        ui::log(format!("Move lksystem to parent cgroup: {:?}", parent_group));
        if let Err(e) = crate::platform::cgroups::move_self_to_cgroup(&parent_group) {
            ui::log(format!("Skipping cgroup cleanup move: {}", e));
            return Ok(());
        }
        let self_cgroup = absolute_v2path.join("lksystem_self");
        ui::log(format!("Remove manager cgroup: {:?}", self_cgroup));
        if let Err(e) = std::fs::remove_dir(&self_cgroup) {
            ui::log(format!("Skipping manager cgroup removal: {}", e));
        }
        ui::log(format!("Remove lksystem managed cgroup: {:?}", absolute_v2path));
        if let Err(e) = std::fs::remove_dir(&absolute_v2path) {
            ui::log(format!("Skipping managed cgroup removal: {}", e));
        }
    }
    Ok(())
}

// base_path should normally be /sys/fs/cgroup
//
// Tries to get the most sensible path to create our own cgroup under.
// Depending on whether cgroupv2 freezing is available It's either a path in
// 1. /sys/fs/cgroup/freezer
// 1. /sys/fs/cgroup/unified
//
// The concrete path will be some sub-directory depending on the cgroup lksystem has been started in
pub fn get_own_freezer(base_path: &std::path::PathBuf) -> Result<std::path::PathBuf, CgroupError> {
    let proc_content = match std::fs::read_to_string("/proc/self/cgroup") {
        Ok(content) => content,
        Err(_) => return Ok(base_path.clone()),
    };
    let proc_content_lines = proc_content.split('\n').collect::<Vec<_>>();
    let v1path = get_own_cgroup_v1(&proc_content_lines);
    let v1_full_path = base_path.join("freezer").join(v1path);
    ui::log(format!("v1 cgroup: {:?}", v1_full_path));
    let v2path = get_own_cgroup_v2(&proc_content_lines);
    // prefer v2 path but fall back to v1 freezer
    let cgroup_path = if let Some(v2path) = v2path {
        let v2_full_path = base_path.join("unified").join(v2path);
        ui::log(format!("v2 cgroup: {:?}", v2_full_path));
        // If v2 group exists but we cant freeze it we still need to use the v1 controller
        if v2_full_path.join("cgroup.freeze").exists() {
            v2_full_path
        } else {
            v1_full_path
        }
    } else {
        v1_full_path
    };
    ui::log(format!("Own cgroup: {:?}", cgroup_path));
    if let Err(e) = fs::create_dir_all(&cgroup_path) {
        ui::log(format!("Skipping cgroup path creation {:?}: {}", cgroup_path, e));
        return Ok(cgroup_path);
    }
    Ok(cgroup_path)
}

// cgroup v2 appears in /proc/self/cgroup as 0::/path/to/cgroup
// but the path is relative to the mount point of cgroups (/sys/fs/cgroup/unified).
pub fn get_own_cgroup_v2(proc_cgroup_content: &[&str]) -> Option<std::path::PathBuf> {
    for line in proc_cgroup_content {
        if line.starts_with("0::") {
            let path = &line[3..];
            // if we are already in the manager cgroup ignore that one. Return the managed cgroup
            let path = path.trim_end_matches(OWN_CGROUP_NAME);
            // ignore leading "/"
            let path = std::path::PathBuf::from(&path[1..]);
            return Some(path);
        }
    }
    None
}

// Try to find the cgroup path for the freezer controller
// If we are in / for freezer find the longest path used in any other cgroup and use that.
//
// cgroups v1 by convention use the same (or a subset) directory trees under each controller so using the
// longest path gives us the most specialized categorization and is probably what others would expect lksystem to do?
fn get_own_cgroup_v1(proc_cgroup_content: &[&str]) -> std::path::PathBuf {
    let mut freezer_path = None;
    let mut longest_path = "/".to_owned();
    for line in proc_cgroup_content {
        let triple = line.split(':').collect::<Vec<_>>();
        if triple.len() == 3 {
            let _id = triple[0];
            let controller = triple[1];
            let path = triple[2];
            if controller.eq("freezer") {
                // ignore leading "/"
                let path = &path[1..];
                freezer_path = Some(std::path::PathBuf::from(path));
            }
            if path.len() > longest_path.len() {
                longest_path = path.to_owned();
            }
        }
    }
    if let Some(p) = freezer_path {
        p
    } else {
        // ignore leading "/"
        std::path::PathBuf::from(&longest_path[1..])
    }
}

// move a process into the cgroup. In lksystem the child process will call move_self for convenience
pub fn move_pid_to_cgroup(
    cgroup_path: &std::path::PathBuf,
    pid: nix::unistd::Pid,
) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        cgroup2::move_pid_to_cgroup(cgroup_path, pid)
    } else {
        cgroup1::move_pid_to_cgroup(cgroup_path, pid)
    }
}

// move this process into the cgroup. Used by lksystem after forking
pub fn move_self_to_cgroup(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        cgroup2::move_self_to_cgroup(cgroup_path)
    } else {
        cgroup1::move_self_to_cgroup(cgroup_path)
    }
}

// retrieve all pids that are currently in this cgroup
pub fn get_all_procs(
    cgroup_path: &std::path::PathBuf,
) -> Result<Vec<nix::unistd::Pid>, CgroupError> {
    let mut pids = Vec::new();
    let cgroup_procs = cgroup_path.join("cgroup.procs");
    let mut f = fs::File::open(&cgroup_procs)
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_procs)))?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_procs)))?;
    for pid_str in buf.split('\n') {
        if pid_str.len() == 0 {
            break;
        }
        if let Ok(pid) = pid_str.parse::<i32>() {
            pids.push(nix::unistd::Pid::from_raw(pid));
        }
    }
    Ok(pids)
}

// kill all processes that are currently in this cgroup.
// This makes sure that the cgroup is first completely frozen
// so all processes will be killed and there is no chance of any
// remaining
pub fn freeze_kill_thaw_cgroup(
    cgroup_path: &std::path::PathBuf,
    sig: nix::sys::signal::Signal,
) -> Result<(), CgroupError> {
    // figure out how to freeze a cgroup so no new processes can be spawned while killing
    let use_v2 = use_v2(cgroup_path);
    ui::log(format!("Freeze cgroup: {:?}", cgroup_path));
    if use_v2 {
        cgroup2::freeze(cgroup_path)?;
        cgroup2::wait_frozen(cgroup_path)?;
    } else {
        cgroup1::freeze(cgroup_path)?;
        cgroup1::wait_frozen(cgroup_path)?;
    }
    ui::log(format!("Kill cgroup: {:?}", cgroup_path));
    kill_cgroup(cgroup_path, sig)?;
    ui::log(format!("Thaw cgroup: {:?}", cgroup_path));
    if use_v2 {
        cgroup2::thaw(cgroup_path)
    } else {
        cgroup1::thaw(cgroup_path)
    }
}

pub fn remove_cgroup(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    fs::remove_dir(&cgroup_path).map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_path)))
}

// kill all processes that are currently in this cgroup.
// You should use wait_frozen before or make in another way sure
// there are no more processes spawned while killing
pub fn kill_cgroup(
    cgroup_path: &std::path::PathBuf,
    sig: nix::sys::signal::Signal,
) -> Result<(), CgroupError> {
    // figure out how to freeze a cgroup so no new processes can be spawned while killing
    let pids = get_all_procs(cgroup_path)?;
    for pid in &pids {
        nix::sys::signal::kill(*pid, sig).map_err(|e| CgroupError::NixErr(e))?;
    }
    Ok(())
}

pub fn wait_frozen(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        cgroup2::wait_frozen(cgroup_path)
    } else {
        cgroup1::wait_frozen(cgroup_path)
    }
}

pub fn freeze(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        cgroup2::freeze(cgroup_path)
    } else {
        cgroup1::freeze(cgroup_path)
    }
}

pub fn thaw(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        cgroup2::thaw(cgroup_path)
    } else {
        cgroup1::thaw(cgroup_path)
    }
}

// Enable controllers on a cgroup parent so child cgroups can use them.
pub fn enable_controllers(
    cgroup_path: &std::path::PathBuf,
    controllers: &Vec<String>,
) -> Result<(), CgroupError> {
    if use_v2(cgroup_path) {
        cgroup2::enable_controllers(cgroup_path, controllers)
    } else {
        // cgroup v1: controllers are per-controller hierarchies; noop here
        Ok(())
    }
}

// Create a scope cgroup for a transient process and move the pid into it.
// Returns the path of the created cgroup.
pub fn create_scope_for_pid(
    scope_name: &str,
    pid: nix::unistd::Pid,
    uid: Option<nix::unistd::Uid>,
) -> Result<std::path::PathBuf, CgroupError> {
    let base = get_own_freezer(&std::path::PathBuf::from("/sys/fs/cgroup"))?;
    let scope_path = if let Some(u) = uid {
        if u.as_raw() != 0 {
            base.join("user.slice").join(format!("user-{}.slice", u.as_raw())).join(format!("{}.scope", scope_name))
        } else {
            base.join("system.slice").join(format!("{}.scope", scope_name))
        }
    } else {
        base.join("system.slice").join(format!("{}.scope", scope_name))
    };
    std::fs::create_dir_all(&scope_path)
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", scope_path)))?;
    // best-effort: enable common controllers on parent
    if let Some(parent) = scope_path.parent() {
        let controllers = vec!["cpu".to_string(), "memory".to_string(), "pids".to_string(), "io".to_string()];
        let _ = enable_controllers(&parent.to_path_buf(), &controllers);
    }
    move_pid_to_cgroup(&scope_path, pid)?;
    Ok(scope_path)
}
