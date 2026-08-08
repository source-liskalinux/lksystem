use crate::ui;
use super::CgroupError;
use std::fs;
use std::io::Write;

// move a process into the cgroup. in lksystem the child process will call move_self for convenience
pub fn move_pid_to_cgroup(
    cgroup_path: &std::path::PathBuf,
    pid: nix::unistd::Pid,
) -> Result<(), CgroupError> {
    let cgroup_procs = cgroup_path.join("cgroup.procs");
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&cgroup_procs)
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_procs)))?;
    let pid_str = pid.as_raw().to_string();
    f.write(pid_str.as_bytes())
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_procs)))?;
    Ok(())
}

// move this process into the cgroup. used by lksystem after forking
pub fn move_self_to_cgroup(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    let pid = nix::unistd::getpid();
    move_pid_to_cgroup(cgroup_path, pid)
}

fn write_freeze_state(
    cgroup_path: &std::path::PathBuf,
    desired_state: &str,
) -> Result<(), CgroupError> {
    let cgroup_freeze = cgroup_path.join("freezer.state");
    if !cgroup_freeze.exists() {
        return Err(CgroupError::IOErr(
            std::io::Error::from(std::io::ErrorKind::NotFound),
            format!("{:?}", cgroup_freeze),
        ));
    }
    ui::log(format!("Write {} to {:?}", desired_state, cgroup_freeze));
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&cgroup_freeze)
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_freeze)))?;
    f.write_all(desired_state.as_bytes())
        .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_freeze)))?;
    Ok(())
}

pub fn wait_frozen(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    let cgroup_freeze = cgroup_path.join("freezer.state");
    loop {
        freeze(cgroup_path)?;
        let content = fs::read_to_string(&cgroup_freeze)
            .map_err(|e| CgroupError::IOErr(e, format!("{:?}", cgroup_freeze)))?;
        if content.starts_with("FROZEN") {
            break;
        } else {
            ui::log(format!("Wait for frozen state. Read (): {}", content));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}

pub fn freeze(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    let desired_state = "FROZEN";
    write_freeze_state(cgroup_path, desired_state)
}

pub fn thaw(cgroup_path: &std::path::PathBuf) -> Result<(), CgroupError> {
    let desired_state = "THAWED";
    write_freeze_state(cgroup_path, desired_state)
}
