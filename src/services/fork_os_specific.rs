use crate::units::PlatformSpecificServiceFields;
use crate::units::ServiceConfig;
use crate::platform::cgroups;
use crate::ui;

/// This is the place to do anything that is not standard unix but specific to one os. Like cgroups
pub fn pre_fork_os_specific(srvc: &ServiceConfig) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = std::fs::create_dir_all(&srvc.platform_specific.cgroup_path) {
            ui::log(format!(
                "Could not create service cgroup {:?}; continuing without cgroups: {}",
                srvc.platform_specific.cgroup_path,
                e
            ));
            return Ok(());
        }
        let mut parent = srvc.platform_specific.cgroup_path.clone();
        if parent.pop() {
            let controllers = vec![
                "cpu".to_string(),
                "memory".to_string(),
                "pids".to_string(),
                "io".to_string(),
            ];
            // ignore errors here; enabling controllers is best-effort
            let _ = crate::platform::cgroups::enable_controllers(&parent, &controllers);
        }
        if let Err(e) = crate::platform::cgroups::apply_service_cgroup_settings(
            &srvc.platform_specific.cgroup_path,
            &srvc.cpu_quota,
            &srvc.cpu_weight,
            &srvc.memory_max,
            &srvc.tasks_max,
            &srvc.io_weight,
        ) {
            ui::log(format!(
                "Could not apply cgroup resource settings for {:?}: {}",
                srvc.platform_specific.cgroup_path,
                e
            ));
        }
    }
    let _ = srvc;
    Ok(())
}

pub fn post_fork_os_specific(conf: &PlatformSpecificServiceFields) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        ui::log(format!("Move service to cgroup: {:?}", &conf.cgroup_path));
        cgroups::move_self_to_cgroup(&conf.cgroup_path)
            .map_err(|e| format!("postfork os specific: {}", e))?;
    }
    let _ = conf;
    Ok(())
}
