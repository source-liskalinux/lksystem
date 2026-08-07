use lksystem::ui;

fn main() {
    let exec_name = std::env::args()
        .next()
        .expect("Could not get executable name from args!");
    let exec_name = std::path::Path::new(&exec_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if exec_name.ends_with("exec_helper") || exec_name.ends_with("lksystem-exec-helper") {
        lksystem::entrypoints::run_exec_helper();
    } else if exec_name.ends_with("lksystem") {
        lksystem::entrypoints::run_service_manager();
    } else {
        ui::error(format!(
            "Can only start as lksystem or exec_helper. Exec name: {}",
            exec_name
        ))
    }
}
