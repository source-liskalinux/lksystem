#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use crate::platform::cgroups::cgroup2;
    fn make_tempdir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("lksystem_test_{}_{}", name, nanos));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn test_cgroup2_move_pid_to_cgroup() {
        let dir = make_tempdir("move_pid");
        let procs = dir.join("cgroup.procs");
        // create writable procs file
        let _ = OpenOptions::new().create(true).write(true).open(&procs).unwrap();
        let pid = nix::unistd::getpid();
        cgroup2::move_pid_to_cgroup(&dir, pid).expect("move pid to cgroup");
        let content = fs::read_to_string(&procs).unwrap();
        assert!(content.contains(&pid.as_raw().to_string()));
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn test_cgroup2_enable_controllers() {
        let dir = make_tempdir("enable_ctrl");
        let sub = dir.join("cgroup.subtree_control");
        fs::write(&sub, "").unwrap();
        let controllers = vec!["cpu".to_string(), "memory".to_string()];
        cgroup2::enable_controllers(&dir, &controllers).expect("enable controllers");
        let content = fs::read_to_string(&sub).unwrap();
        assert!(content.contains("cpu"));
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn test_cgroup2_freeze_thaw() {
        let dir = make_tempdir("freeze");
        let freeze_file = dir.join("cgroup.freeze");
        fs::write(&freeze_file, "0").unwrap();
        cgroup2::freeze(&dir).expect("freeze");
        let c = fs::read_to_string(&freeze_file).unwrap();
        assert!(c.starts_with('1') || c.starts_with("1"));
        cgroup2::thaw(&dir).expect("thaw");
        let c2 = fs::read_to_string(&freeze_file).unwrap();
        assert!(c2.starts_with('0') || c2.starts_with("0"));
        let _ = fs::remove_dir_all(&dir);
    }
}
