use crate::ui;
use which::which;
use super::fork_child;
use crate::fd_store::FDStore;
use crate::services::RunCmdError;
use crate::services::Service;
use crate::units::ServiceConfig;
use std::path::Path;

fn build_hyprland_environment() -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Ok(display) = std::env::var("DISPLAY") {
        env.push(("DISPLAY".to_owned(), display));
    }
    if let Ok(display) = std::env::var("WAYLAND_DISPLAY") {
        env.push(("WAYLAND_DISPLAY".to_owned(), display));
    }
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        env.push(("XDG_RUNTIME_DIR".to_owned(), runtime_dir));
    }
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        env.push(("XDG_SESSION_TYPE".to_owned(), session_type));
    }
    if let Ok(current_desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        env.push(("XDG_CURRENT_DESKTOP".to_owned(), current_desktop));
    }
    if let Ok(desktop_session) = std::env::var("DESKTOP_SESSION") {
        env.push(("DESKTOP_SESSION".to_owned(), desktop_session));
    }
    if let Ok(seat) = std::env::var("XDG_SEAT") {
        env.push(("XDG_SEAT".to_owned(), seat));
    }
    if let Ok(vtnr) = std::env::var("XDG_VTNR") {
        env.push(("XDG_VTNR".to_owned(), vtnr));
    }
    if let Ok(home) = std::env::var("HOME") {
        env.push(("HOME".to_owned(), home));
    }
    if let Ok(dbus_address) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        env.push(("DBUS_SESSION_BUS_ADDRESS".to_owned(), dbus_address));
    }
    env
}

fn build_service_environment(
    environment: &Option<crate::units::EnvVars>,
    environment_files: &[std::path::PathBuf],
    notifications_path: &str,
    listener_names: &[String],
) -> Vec<(String, String)> {
    let mut env = vec![
        ("LISTEN_FDS".to_owned(), format!("{}", listener_names.len())),
        ("LISTEN_FDNAMES".to_owned(), listener_names.join(":")),
        ("NOTIFY_SOCKET".to_owned(), notifications_path.to_owned()),
    ];
    env.extend(build_hyprland_environment());
    if let Some(env_vars) = environment {
        env.extend(env_vars.vars.iter().cloned());
    }
    for env_file in environment_files {
        if let Ok(contents) = std::fs::read_to_string(env_file) {
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    env.push((k.trim().to_owned(), v.trim().to_owned()));
                }
            }
        }
    }
    env
}

fn start_service_with_filedescriptors(
    self_path: &Path,
    srvc: &mut Service,
    conf: &ServiceConfig,
    name: &str,
    fd_store: &FDStore,
) -> Result<(), RunCmdError> {
    // check if executable even exists
    let cmd = which(&conf.exec.cmd).map_err(|err| {
        RunCmdError::SpawnError(
            name.to_owned(),
            format!("Could not resolve command to an exectuable file: {err:?}"),
        )
    })?;
    if !cmd.exists() {
        ui::error(format!(
            "The service {} specified an executable that does not exist: {:?}",
            name, &conf.exec.cmd
        ));
        return Err(RunCmdError::SpawnError(
            conf.exec.cmd.clone(),
            format!("Executable does not exist"),
        ));
    }
    if !cmd.is_file() {
        ui::error(format!(
            "The service {} specified an executable that is not a file: {:?}",
            name, &cmd
        ));
        return Err(RunCmdError::SpawnError(
            conf.exec.cmd.clone(),
            format!("Executable does not exist (is a directory)"),
        ));
    }
    // 1. fork
    // 1. in fork use dup2 to map all relevant file desrciptors to 3..x
    // 1. in fork mark all other file descriptors with FD_CLOEXEC
    // 1. in fork set relevant env varibales $LISTEN_FDS $LISTEN_PID
    // 1. in fork execve the cmd with the args
    // 1. in parent set pid and return. Waiting will be done afterwards if necessary
    let notifications_path = {
        if let Some(p) = &srvc.notifications_path {
            p.to_str().unwrap().to_owned()
        } else {
            return Err(RunCmdError::Generic(format!(
                "Tried to start service: {} without a notifications path",
                name,
            )));
        }
    };
    super::fork_os_specific::pre_fork_os_specific(conf).map_err(|e| RunCmdError::Generic(e))?;
    let mut fds = Vec::new();
    let mut names = Vec::new();
    for socket in &conf.sockets {
        let sock_fds = fd_store
            .get_global(&socket.name)
            .unwrap()
            .iter()
            .map(|(_, _, fd)| fd.as_raw_fd())
            .collect::<Vec<_>>();
        let sock_names = fd_store
            .get_global(&socket.name)
            .unwrap()
            .iter()
            .map(|(_, name, _)| name.clone())
            .collect::<Vec<_>>();
        fds.extend(sock_fds);
        names.extend(sock_names);
    }
    // We first exec into our own executable again and apply this config
    // We transfer the config via a anonymous shared memory file
    let env = build_service_environment(
        &conf.exec_config.environment,
        &conf.exec_config.environment_files,
        &notifications_path,
        &names,
    );
    let exec_helper_conf = crate::entrypoints::ExecHelperConfig {
        name: name.to_owned(),
        cmd: cmd,
        args: conf.exec.args.clone(),
        env,
        group: conf.exec_config.group.as_raw(),
        supplementary_groups: conf
            .exec_config
            .supplementary_groups
            .iter()
            .map(|gid| gid.as_raw())
            .collect(),
        user: conf.exec_config.user.as_raw(),
        working_directory: conf.exec_config.working_directory.clone(),
        platform_specific: conf.platform_specific.clone(),
    };
    let marshalled_config = serde_json::to_string(&exec_helper_conf).unwrap();
    // crate the shared memory file
    let exec_helper_conf_fd = shmemfdrs::create_shmem(
        &std::ffi::CString::new(name).unwrap(),
        marshalled_config.as_bytes().len() + 1,
    );
    if exec_helper_conf_fd < 0 {
        return Err(RunCmdError::CreatingShmemFailed(
            name.to_owned(),
            std::io::Error::from_raw_os_error(exec_helper_conf_fd).kind(),
        ));
    }
    use std::os::unix::io::FromRawFd;
    let mut exec_helper_conf_file = unsafe { std::fs::File::from_raw_fd(exec_helper_conf_fd) };
    // write the config to the file
    use std::io::Write;
    exec_helper_conf_file
        .write_all(marshalled_config.as_bytes())
        .unwrap();
    exec_helper_conf_file.write(&[b'\n']).unwrap();
    use std::io::Seek;
    exec_helper_conf_file
        .seek(std::io::SeekFrom::Start(0))
        .unwrap();
    // need to allocate this before forking. Currently this is just static info, we could only do this once...
    let self_path_cstr = std::ffi::CString::new(self_path.to_str().unwrap()).unwrap();
    let name_arg = std::ffi::CString::new("exec_helper").unwrap();
    let self_args = [name_arg.as_ptr(), std::ptr::null()];
    ui::log(format!("Start main executable for service: {name}: {:?} {:?}", exec_helper_conf.cmd, exec_helper_conf.args));
    match unsafe { nix::unistd::fork() } {
        Ok(nix::unistd::ForkResult::Parent { child, .. }) => {
            // make sure the file exists until after we fork before closing it
            drop(exec_helper_conf_file);
            srvc.pid = Some(child);
            srvc.process_group = Some(nix::unistd::Pid::from_raw(-child.as_raw()));
        }
        Ok(nix::unistd::ForkResult::Child) => {
            let stdout = {
                if let Some(stdio) = &srvc.stdout {
                    stdio.write_fd()
                } else {
                    unreachable!();
                }
            };
            let stderr = {
                if let Some(stdio) = &srvc.stderr {
                    stdio.write_fd()
                } else {
                    unreachable!();
                }
            };
            fork_child::after_fork_child(
                &self_path_cstr,
                self_args.as_slice(),
                &mut fds,
                stdout,
                stderr,
                exec_helper_conf_fd,
            );
        }
        Err(e) => ui::error(format!("Fork for service: {} failed with: {}", name, e)),
    }
    Ok(())
}

pub fn start_service(
    self_path: &Path,
    srvc: &mut Service,
    conf: &ServiceConfig,
    name: &str,
    fd_store: &FDStore,
) -> Result<(), super::RunCmdError> {
    start_service_with_filedescriptors(self_path, srvc, conf, name, fd_store)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_service_environment;
    use crate::units::EnvVars;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn build_service_environment_includes_unit_and_file_values() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("lksystem-hyprland-{unique}"));
        fs::create_dir_all(&temp_dir).unwrap();
        let env_file = temp_dir.join("envfile.env");
        fs::write(&env_file, "XDG_RUNTIME_DIR=/tmp/runtime\nWAYLAND_DISPLAY=wayland-1\n").unwrap();
        unsafe {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/dbus-session");
            std::env::set_var("XDG_RUNTIME_DIR", "/tmp/runtime-session");
            std::env::set_var("WAYLAND_DISPLAY", "wayland-session");
            std::env::set_var("XDG_CURRENT_DESKTOP", "Hyprland");
        }
        let environment = Some(EnvVars {
            vars: vec![
                ("XDG_CURRENT_DESKTOP".to_owned(), "Hyprland".to_owned()),
            ],
        });
        let env = build_service_environment(
            &environment,
            &[env_file.clone()],
            "/run/user/1000/bus",
            &["fd1".to_owned()],
        );
        let env_map: std::collections::HashMap<_, _> = env.into_iter().collect();
        assert_eq!(
            env_map.get("DBUS_SESSION_BUS_ADDRESS"),
            Some(&"unix:path=/tmp/dbus-session".to_string())
        );
        assert_eq!(env_map.get("XDG_CURRENT_DESKTOP"), Some(&"Hyprland".to_string()));
        assert_eq!(env_map.get("XDG_RUNTIME_DIR"), Some(&"/tmp/runtime".to_string()));
        assert_eq!(env_map.get("WAYLAND_DISPLAY"), Some(&"wayland-1".to_string()));
        assert_eq!(env_map.get("LISTEN_FDS"), Some(&"1".to_string()));
        assert_eq!(env_map.get("NOTIFY_SOCKET"), Some(&"/run/user/1000/bus".to_string()));
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
