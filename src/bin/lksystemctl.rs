use lksystem::control::jsonrpc2::Call;
use lksystem::ui;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
        print_help();
        return;
    }
    // pure enable / disable / is-enabled / show / cat filesystem operation.
    // doesn't need active daemon.
    match args[0].as_str() {
        "enable" => {
            if args.len() < 2 {
                ui::error("Usage: lksystemctl enable <unit> [unit...]");
                std::process::exit(1);
            }
            for name in &args[1..] {
                if let Err(e) = enable_unit(name) {
                    ui::error(&format!("Failed to enable {name}: {e}"));
                    std::process::exit(1);
                }
            }
            return;
        }
        "disable" => {
            if args.len() < 2 {
                ui::error("Usage: lksystemctl disable <unit> [unit...]");
                std::process::exit(1);
            }
            for name in &args[1..] {
                if let Err(e) = disable_unit(name) {
                    ui::error(&format!("Failed to disable {name}: {e}"));
                    std::process::exit(1);
                }
            }
            return;
        }
        "is-enabled" => {
            let Some(name) = args.get(1) else {
                ui::error("Usage: lksystemctl is-enabled <unit>");
                std::process::exit(1);
            };
            match is_enabled_unit(name) {
                Ok(true) => ui::write_line("enabled"),
                Ok(false) => {
                    ui::write_line("disabled");
                    std::process::exit(1);
                }
                Err(e) => {
                    ui::error(&format!("Failed to check {name}: {e}"));
                    std::process::exit(1);
                }
            }
            return;
        }
        "cat" => {
            let Some(name) = args.get(1) else {
                ui::error("Usage: lksystemctl cat <unit>");
                std::process::exit(1);
            };
            match resolve_unit_path(name) {
                Ok(path) => match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        ui::write_line(format!("# {}", path.display()));
                        ui::write(content);
                    }
                    Err(e) => {
                        ui::error(&format!("Failed to read {}: {e}", path.display()));
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    ui::error(&e);
                    std::process::exit(1);
                }
            }
            return;
        }
        "show" => {
            let Some(name) = args.get(1) else {
                ui::error("Usage: lksystemctl show <unit>");
                std::process::exit(1);
            };
            if let Err(e) = show_unit(name) {
                ui::error(&e);
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }
    let (method, params) = match args[0].as_str() {
        "status" => {
            let unit = args.get(1).cloned();
            ("status".to_string(), unit.map(Value::String))
        }
        "is-active" => {
            let unit = if args.len() < 2 {
                ui::error("Usage: lksystemctl is-active <unit>");
                std::process::exit(1);
            } else {
                args[1].clone()
            };
            ("status".to_string(), Some(Value::String(unit)))
        }
        "start" => {
            if args.len() < 2 {
                ui::error("Usage: lksystemctl start <unit>");
                std::process::exit(1);
            }
            ("start".to_string(), Some(Value::String(args[1].clone())))
        }
        "stop" => {
            if args.len() < 2 {
                ui::error("Usage: lksystemctl stop <unit>");
                std::process::exit(1);
            }
            ("stop".to_string(), Some(Value::String(args[1].clone())))
        }
        "restart" => {
            if args.len() < 2 {
                ui::error("Usage: lksystemctl restart <unit>");
                std::process::exit(1);
            }
            ("restart".to_string(), Some(Value::String(args[1].clone())))
        }
        "list-units" | "list_units" | "list" => {
            let kind = args.get(1).cloned();
            ("list-units".to_string(), kind.map(Value::String))
        }
        "reload" | "daemon-reload" => ("reload".to_string(), None),
        "shutdown" | "poweroff" => ("poweroff".to_string(), None),
        "reboot" => ("reboot".to_string(), None),
        "halt" => ("halt".to_string(), None),
        other => {
            ui::error(&format!("Unsupported command: {other}"));
            print_help();
            return;
        }
    };
    let addr = std::env::var("LKSYSTEMCTL_ADDR").unwrap_or_else(|_| {
        std::path::PathBuf::from("./notifications/control.socket")
            .to_string_lossy()
            .into_owned()
    });
    let call = Call { method: method.clone(), params: params.clone(), id: None };
    let payload = serde_json::to_string(&call.to_json()).unwrap();
    let response: Value = if use_dbus_transport() {
        send_dbus_command(&method, params)
    } else {
        send_jsonrpc_command(&addr, &payload)
    };
    handle_response(&args[0], &response);
}

fn use_dbus_transport() -> bool {
    std::env::var("LKSYSTEMCTL_TRANSPORT")
        .map(|value| value.eq_ignore_ascii_case("dbus"))
        .unwrap_or(false)
}

fn send_jsonrpc_command(addr: &str, payload: &str) -> Value {
    if addr.starts_with('/') {
        let mut stream = std::os::unix::net::UnixStream::connect(&addr).unwrap_or_else(|e| {
            ui::error(&format!("Failed to connect to lksystem control socket at {addr}: {e}"));
            ui::write_error("Is lksystem running?");
            std::process::exit(1);
        });
        stream.write_all(payload.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        serde_json::from_reader(&mut stream).unwrap_or_else(|e| {
            ui::error(&format!("Failed to parse control response: {e}"));
            std::process::exit(1);
        })
    } else {
        let mut stream = std::net::TcpStream::connect(&addr).unwrap_or_else(|e| {
            ui::error(&format!("Failed to connect to lksystem control socket at {addr}: {e}"));
            ui::write_error("Is lksystem running?");
            std::process::exit(1);
        });
        stream.write_all(payload.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        serde_json::from_reader(&mut stream).unwrap_or_else(|e| {
            ui::error(&format!("Failed to parse control response: {e}"));
            std::process::exit(1);
        })
    }
}

fn send_dbus_command(method: &str, params: Option<Value>) -> Value {
    use dbus::blocking::Connection;
    use std::time::Duration;
    let connection = Connection::new_system().unwrap_or_else(|e| {
        ui::error(&format!("Failed to connect to system D-Bus: {e}"));
        std::process::exit(1);
    });
    let proxy = connection.with_proxy(
        "org.lksystem.Control",
        "/org/lksystem/Control",
        Duration::from_secs(3),
    );
    let arg = params
        .and_then(|p| p.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    match method {
        "status" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Status", (arg,))
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Status call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "list-units" | "list_units" | "list" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "ListUnits", (arg,))
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus ListUnits call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "start" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Start", (arg,))
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Start call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "stop" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Stop", (arg,))
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Stop call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "restart" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Restart", (arg,))
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Restart call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "reload" | "daemon-reload" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Reload", (arg,))
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Reload call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "shutdown" | "poweroff" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Poweroff", ())
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Poweroff call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "reboot" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Reboot", ())
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Reboot call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        "halt" => {
            let (reply,): (String,) = proxy
                .method_call("org.lksystem.Control", "Halt", ())
                .unwrap_or_else(|e| {
                    ui::error(&format!("D-Bus Halt call failed: {e}"));
                    std::process::exit(1);
                });
            Value::String(reply)
        }
        _ => {
            ui::error(&format!("Unsupported command for D-Bus transport: {method}"));
            std::process::exit(1);
        }
    }
}

fn handle_response(command: &str, response: &Value) {
    if let Some(error) = response.get("error") {
        ui::error(&format!("Control command failed: {}", error));
        std::process::exit(1);
    }
    let result = response.get("result").unwrap_or(&Value::Null);
    match command {
        "is-active" => {
            let active = is_active_result(result);
            ui::write_line(if active { "active" } else { "inactive" });
            std::process::exit(if active { 0 } else { 3 });
        }
        "list-units" | "list" | "list_units" => {
            if let Some(list) = result.as_array() {
                for item in list {
                    if let Some(name) = item.as_str() {
                        ui::write_line(name);
                    }
                }
            } else if let Some(text) = result.as_str() {
                for line in text.lines() {
                    ui::write_line(line);
                }
            }
        }
        "status" => {
            print_status_result(result);
        }
        _ => {
            if let Some(text) = result.as_str() {
                ui::write_line(text);
            } else if !result.is_null() && !result.as_array().map_or(true, |arr| arr.is_empty()) {
                ui::write_line(serde_json::to_string_pretty(result).unwrap());
            }
        }
    }
}

fn is_active_result(result: &Value) -> bool {
    if let Some(array) = result.as_array() {
        if let Some(first) = array.first() {
            if let Some(status) = first.get("Status").and_then(|v| v.as_str()) {
                return status.starts_with("Started");
            }
        }
    }
    if let Some(text) = result.as_str() {
        return text.contains("active");
    }
    false
}

fn print_status_result(result: &Value) {
    if let Some(array) = result.as_array() {
        for item in array {
            if let Some(name) = item.get("Name").and_then(|v| v.as_str()) {
                if let Some(status) = item.get("Status").and_then(|v| v.as_str()) {
                    ui::write(format!("{name} {status}"));
                    let extras: Vec<String> = item
                        .as_object()
                        .unwrap_or(&serde_json::Map::new())
                        .iter()
                        .filter_map(|(k, v)| {
                            if k == "Name" || k == "Status" {
                                None
                            } else {
                                Some(format!(" {}={}", k, v))
                            }
                        })
                        .collect();
                    for extra in extras {
                        ui::write(extra);
                    }
                    ui::write_line("");
                } else {
                    ui::write_line(item);
                }
            } else {
                ui::write_line(item);
            }
        }
    } else {
        ui::write_line(result);
    }
}

fn unit_dirs() -> Vec<PathBuf> {
    let (_, conf) = lksystem::config::load_config(&None);
    match conf {
        Ok(c) if !c.unit_dirs.is_empty() => c.unit_dirs,
        _ => vec![PathBuf::from("./test_units")],
    }
}

fn resolve_unit_path(name: &str) -> Result<PathBuf, String> {
    let candidates: Vec<String> = if name.ends_with(".service") || name.ends_with(".socket") || name.ends_with(".target") {
        vec![name.to_string()]
    } else {
        vec![format!("{name}.service"), format!("{name}.socket"), format!("{name}.target")]
    };
    for dir in unit_dirs() {
        for cand in &candidates {
            let p = dir.join(cand);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    Err(format!("Unit {name} not found in any of {:?}", unit_dirs()))
}

fn parse_install_section(path: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut in_install = false;
    let mut wanted_by = Vec::new();
    let mut required_by = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line.eq_ignore_ascii_case("[install]");
            continue;
        }
        if !in_install {
            continue;
        }
        if let Some(val) = line.strip_prefix("WantedBy=").or_else(|| line.strip_prefix("wantedby=")) {
            wanted_by.extend(val.split_whitespace().map(|s| s.to_string()));
        } else if let Some(val) = line.strip_prefix("RequiredBy=").or_else(|| line.strip_prefix("requiredby=")) {
            required_by.extend(val.split_whitespace().map(|s| s.to_string()));
        }
    }
    Ok((wanted_by, required_by))
}

fn wants_dirs_for(unit_path: &Path, targets: &[String], suffix: &str) -> Vec<PathBuf> {
    let base = unit_dirs().into_iter().next().unwrap_or_else(|| {
        unit_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    });
    targets.iter().map(|t| base.join(format!("{t}{suffix}"))).collect()
}

fn enable_unit(name: &str) -> Result<(), String> {
    let unit_path = resolve_unit_path(name)?;
    let (wanted_by, required_by) = parse_install_section(&unit_path)?;
    if wanted_by.is_empty() && required_by.is_empty() {
        ui::warning("The unit files have no installation config [ WantedBy= / RequiredBy= ], and make no sense to enable.");
        return Ok(());
    }
    let file_name = unit_path.file_name().ok_or("invalid unit path")?;
    let mut created_any = false;
    for dir in wants_dirs_for(&unit_path, &wanted_by, ".wants") {
        create_symlink(&dir, file_name, &unit_path)?;
        created_any = true;
    }
    for dir in wants_dirs_for(&unit_path, &required_by, ".requires") {
        create_symlink(&dir, file_name, &unit_path)?;
        created_any = true;
    }
    if created_any {
        ui::success(&format!("Created symlink(s) for {}.", unit_path.display()));
    }
    Ok(())
}

fn create_symlink(dir: &Path, file_name: &std::ffi::OsStr, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let link_path = dir.join(file_name);
    if link_path.symlink_metadata().is_ok() {
        return Ok(());
    }
    let target_for_link = pathdiff(target, dir).unwrap_or_else(|| target.to_path_buf());
    std::os::unix::fs::symlink(&target_for_link, &link_path)
        .map_err(|e| format!("symlink {} -> {}: {e}", link_path.display(), target_for_link.display()))?;
    ui::write_line(format!(" {} -> {}", link_path.display(), target_for_link.display()));
    Ok(())
}

fn pathdiff(to_file: &Path, from_dir: &Path) -> Option<PathBuf> {
    let to_abs = std::fs::canonicalize(to_file).ok()?;
    let from_abs = std::fs::canonicalize(from_dir).ok()?;
    let to_comps: Vec<_> = to_abs.components().collect();
    let from_comps: Vec<_> = from_abs.components().collect();
    let common = to_comps.iter().zip(from_comps.iter()).take_while(|(a, b)| a == b).count();
    let mut rel = PathBuf::new();
    for _ in common..from_comps.len() {
        rel.push("..");
    }
    for comp in &to_comps[common..] {
        rel.push(comp.as_os_str());
    }
    Some(rel)
}

fn disable_unit(name: &str) -> Result<(), String> {
    let unit_path = resolve_unit_path(name)?;
    let file_name = unit_path.file_name().ok_or("invalid unit path")?.to_owned();
    let mut removed_any = false;
    for dir in unit_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            let is_wants_dir = p.is_dir()
                && p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(".wants") || n.ends_with(".requires")).unwrap_or(false);
            if !is_wants_dir {
                continue;
            }
            let link = p.join(&file_name);
            if link.symlink_metadata().is_ok() {
                std::fs::remove_file(&link).map_err(|e| format!("removing {}: {e}", link.display()))?;
                ui::write_line(format!("Removed {}", link.display()));
                removed_any = true;
            }
        }
    }
    if !removed_any {
        ui::write_line(format!("{} is not enabled!", unit_path.display()));
    }
    Ok(())
}

fn is_enabled_unit(name: &str) -> Result<bool, String> {
    let unit_path = resolve_unit_path(name)?;
    let file_name = unit_path.file_name().ok_or("invalid unit path")?;
    for dir in unit_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            let is_wants_dir = p.is_dir()
                && p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(".wants") || n.ends_with(".requires")).unwrap_or(false);
            if is_wants_dir && p.join(file_name).symlink_metadata().is_ok() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn show_unit(name: &str) -> Result<(), String> {
    let unit_path = resolve_unit_path(name)?;
    let content = std::fs::read_to_string(&unit_path).map_err(|e| format!("{}: {e}", unit_path.display()))?;
    let mut section = String::new();
    ui::write_line(format!("# Reduced property view of {} (raw directives, not live daemon state)", unit_path.display()));
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).to_string();
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            ui::write_line(format!("{section}.{k}={v}"));
        }
    }
    let enabled = is_enabled_unit(name)?;
    ui::write_line(format!("UnitFileState={}", if enabled { "enabled" } else { "disabled" }));
    Ok(())
}

fn print_help() {
    ui::write_line("");
    ui::write_line("-------------------------------");
    ui::write_line("::: [ LKSYSTEMCTL (1.0.0) ] :::");
    ui::write_line("-------------------------------");
    ui::write_line("");
    ui::write_line("Usage: lksystemctl <command> [unit]");
    ui::write_line("> status [unit]                                                        check the status of a unit or all units");
    ui::write_line("> is-active [unit]                                                     check if unit is active");
    ui::write_line("> start <unit>                                                         start a unit");
    ui::write_line("> stop <unit>                                                          stop a unit");
    ui::write_line("> restart <unit>                                                       restart a unit");
    ui::write_line("> poweroff | shutdown                                                  power off the system");
    ui::write_line("> reboot                                                               reboot the system");
    ui::write_line("> halt                                                                 halt the system");
    ui::write_line("> list | list-units [service|socket|target|device|timer|mount|path]    show all units or units of a specific type");
    ui::write_line("> reload | daemon-reload                                               reload unit files");
    ui::write_line("> enable <unit> [unit]                                                 enable unit(s) (create symlinks)");
    ui::write_line("> disable <unit> [unit]                                                disable unit(s) (remove symlinks)");
    ui::write_line("> is-enabled <unit>                                                    check if unit is enabled (symlink exists)");
    ui::write_line("> show <unit>                                                          show unit file directives (raw, not live daemon state)");
    ui::write_line("> cat <unit>                                                           show unit file content (raw, not live daemon state)");
    ui::write_line("");
}
