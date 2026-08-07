//! Config can be loaded from env vars, lua, or json.
//!
//! The preferred runtime configuration format is a Lua script returning a table.
//! A stronger state backend is initialized with SQLite during early startup.
//!
//! Currently configurable:
//! ### Logging
//! 1. Whether or not to log to disk (and the dir to put the logs in)
//! 2. Whether or not to log to stdout
//!
//! ### General config
//! 1. Where to find the units (one or more directories)
//! 2. notification-socket directory (where the unix-domain sockets are placed on which services can notify lksystem)
//! 3. Which unit is the target that should be started
//! 4. Optional SQLite state DB path

use mlua::{Lua, Table, Value as LuaValue};
use rusqlite::Connection;
use serde_json;
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Debug)]
pub struct LoggingConfig {
    pub log_to_stdout: bool,
    pub log_to_disk: bool,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub unit_dirs: Vec<PathBuf>,
    pub target_unit: String,
    pub notification_sockets_dir: PathBuf,
    pub self_path: PathBuf,
    pub sqlite_db_path: PathBuf,
}

#[derive(Debug)]
enum SettingValue {
    Str(String),
    Array(Vec<SettingValue>),
    Boolean(bool),
}

fn load_lua(
    config_path: &PathBuf,
    settings: &mut HashMap<String, SettingValue>,
) -> Result<(), String> {
    let config_content = fs::read_to_string(config_path)
        .map_err(|e| format!("Error while opening config file: {}", e))?;
    let lua = Lua::new();
    let value = lua
        .load(&config_content)
        .set_name(&config_path.to_string_lossy()[..])
        .eval::<LuaValue>()
        .map_err(|e| format!("Error while decoding config lua: {}", e))?;
    let table = match value {
        LuaValue::Table(table) => table,
        _ => lua
            .globals()
            .get::<Option<Table>>("config")
            .map_err(|e| format!("Error while decoding config lua: {}", e))?
            .ok_or_else(|| {
                "Error while decoding config lua: expected a table or global `config` table".to_string()
            })?,
    };
    if let Some(dirs) = get_lua_string_array(&table, "unit_dirs")? {
        settings.insert(
            "unit.dirs".to_owned(),
            SettingValue::Array(dirs.into_iter().map(SettingValue::Str).collect()),
        );
    }
    if let Some(logging_dir) = get_lua_string(&table, "logging_dir")? {
        settings.insert("logging.dir".to_owned(), SettingValue::Str(logging_dir));
    }
    if let Some(val) = get_lua_bool(&table, "log_to_disk")? {
        settings.insert("logging.to.disk".to_owned(), SettingValue::Boolean(val));
    }
    if let Some(val) = get_lua_bool(&table, "log_to_stdout")? {
        settings.insert("logging.to.stdout".to_owned(), SettingValue::Boolean(val));
    }
    if let Some(target_unit) = get_lua_string(&table, "target_unit")? {
        settings.insert("target.unit".to_owned(), SettingValue::Str(target_unit));
    }
    if let Some(selfpath) = get_lua_string(&table, "selfpath")? {
        settings.insert("selfpath".to_owned(), SettingValue::Str(selfpath));
    }
    if let Some(notifications_dir) = get_lua_string(&table, "notifications_dir")? {
        settings.insert(
            "notifications.dir".to_owned(),
            SettingValue::Str(notifications_dir),
        );
    }
    if let Some(sqlite_db) = get_lua_string(&table, "sqlite_db")? {
        settings.insert(
            "sqlite.db".to_owned(),
            SettingValue::Str(sqlite_db),
        );
    }
    Ok(())
}

fn setting_as_bool(val: &SettingValue) -> Option<bool> {
    match val {
        SettingValue::Boolean(b) => Some(*b),
        // Env var & JSON string values datang sebagai SettingValue::Str, mis.
        // "true"/"false"/"1"/"0" -- versi lama HANYA menerima SettingValue::Boolean
        // dan diam-diam menganggap semua yang lain `false`, jadi
        // `LKSYSTEM_LOGGING_TO_STDOUT=true` tidak pernah benar-benar berpengaruh.
        SettingValue::Str(s) => match s.to_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        },
        SettingValue::Array(_) => None,
    }
}

fn load_json(
    config_path: &PathBuf,
    settings: &mut HashMap<String, SettingValue>,
) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|e| format!("Error while opening config file: {}", e))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Error while decoding config json: {}", e))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "Error while decoding config json: expected a top-level object".to_string())?;

    let mut put_str = |key: &str, settings_key: &str| {
        if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
            settings.insert(settings_key.to_owned(), SettingValue::Str(s.to_owned()));
        }
    };
    put_str("logging_dir", "logging.dir");
    put_str("target_unit", "target.unit");
    put_str("selfpath", "selfpath");
    put_str("notifications_dir", "notifications.dir");
    put_str("sqlite_db", "sqlite.db");

    if let Some(b) = obj.get("log_to_disk").and_then(|v| v.as_bool()) {
        settings.insert("logging.to.disk".to_owned(), SettingValue::Boolean(b));
    }
    if let Some(b) = obj.get("log_to_stdout").and_then(|v| v.as_bool()) {
        settings.insert("logging.to.stdout".to_owned(), SettingValue::Boolean(b));
    }
    if let Some(arr) = obj.get("unit_dirs").and_then(|v| v.as_array()) {
        let dirs: Vec<SettingValue> = arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| SettingValue::Str(s.to_owned()))
            .collect();
        settings.insert("unit.dirs".to_owned(), SettingValue::Array(dirs));
    }
    Ok(())
}

fn get_lua_string(table: &Table, key: &str) -> Result<Option<String>, String> {
    table
        .get::<Option<String>>(key)
        .map_err(|e| format!("Error while decoding config lua: {}", e))
}

fn get_lua_bool(table: &Table, key: &str) -> Result<Option<bool>, String> {
    table
        .get::<Option<bool>>(key)
        .map_err(|e| format!("Error while decoding config lua: {}", e))
}

fn get_lua_string_array(table: &Table, key: &str) -> Result<Option<Vec<String>>, String> {
    match table.get::<LuaValue>(key) {
        Ok(LuaValue::Table(list)) => {
            let mut values = Vec::new();
            for item in list.sequence_values::<String>() {
                values.push(item.map_err(|e| format!("Error while decoding config lua: {}", e))?);
            }
            Ok(Some(values))
        }
        Ok(LuaValue::Nil) => Ok(None),
        Ok(_) => Err(format!(
            "Error while decoding config lua: {} must be a list of strings",
            key
        )),
        Err(e) => Err(format!("Error while decoding config lua: {}", e)),
    }
}

fn initialize_sqlite_db(db_path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create db.sqlite directory: {}", e))?;
    }
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Error while opening db.sqlite: {}", e))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS unit_state (
            unit_name TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_state TEXT,
            last_exit_code INTEGER,
            last_updated INTEGER
        )",
        [],
    )
    .map_err(|e| format!("Error while initializing sqlite schema: {}", e))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS config_meta (
            key TEXT PRIMARY KEY,
            value TEXT
        )",
        [],
    )
    .map_err(|e| format!("Error while initializing sqlite schema: {}", e))?;
    Ok(())
}

fn resolve_relative_path(path: &PathBuf, config_dir: &PathBuf) -> PathBuf {
    if path.is_relative() {
        config_dir.join(path)
    } else {
        path.clone()
    }
}

pub fn load_config(config_path: &Option<PathBuf>) -> (LoggingConfig, Result<Config, String>) {
    let mut settings: HashMap<String, SettingValue> = HashMap::new();
    const LUA_CONFIG_FILENAME: &str = "config.lua";
    const JSON_CONFIG_FILENAME: &str = "config.json";
    let default_config_dir = if let Some(config_path) = config_path {
        config_path.clone()
    } else if let Ok(env_dir) = std::env::var("LKSYSTEM_CONFIG_DIR") {
        if !env_dir.is_empty() {
            PathBuf::from(env_dir)
        } else {
            PathBuf::from("/etc/lksystem")
        }
    } else {
        let etc_config = PathBuf::from("/etc/lksystem");
        if etc_config.exists() {
            etc_config
        } else {
            PathBuf::from("./config")
        }
    };

    // PENTING: file config (Lua/JSON) HARUS dimuat ke `settings` SEBELUM kita
    // membaca nilai apa pun darinya di bawah -- versi lama menyimpan
    // hasil load_lua()/load_json() cuma untuk dicek "apakah sukses", TAPI baru
    // dilakukan SETELAH semua field Config sudah diekstrak dari `settings`.
    // Akibatnya isi file config tidak pernah benar-benar terpakai, hanya
    // env var yang berpengaruh. Urutan yang benar: file dulu (basis), lalu
    // env var menimpa di atasnya (env selalu menang, sesuai konvensi umum).
    let lua_path = default_config_dir.join(LUA_CONFIG_FILENAME);
    let json_path = default_config_dir.join(JSON_CONFIG_FILENAME);
    let lua_conf: Option<Result<(), String>> = if lua_path.is_file() {
        Some(load_lua(&lua_path, &mut settings))
    } else {
        None
    };
    let json_conf: Option<Result<(), String>> = if json_path.is_file() {
        Some(load_json(&json_path, &mut settings))
    } else {
        None
    };

    std::env::vars().for_each(|(key, value)| {
        let mut new_key: Vec<String> = key.split('_').map(|part| part.to_lowercase()).collect();
        if new_key.first().is_some_and(|prefix| prefix == "lksystem") {
            new_key.remove(0);
            let new_key = new_key.join(".");
            settings.insert(new_key, SettingValue::Str(value));
        }
    });
    let log_dir = settings.get("logging.dir").and_then(|dir| match dir {
        SettingValue::Str(s) => Some(resolve_relative_path(&PathBuf::from(s), &default_config_dir)),
        _ => None,
    });
    let log_to_stdout = settings.get("logging.to.stdout").and_then(setting_as_bool);
    let log_to_disk = settings.get("logging.to.disk").and_then(setting_as_bool);
    let notification_sockets_dir = settings.get("notifications.dir").and_then(|dir| match dir {
        SettingValue::Str(s) => Some(resolve_relative_path(&PathBuf::from(s), &default_config_dir)),
        _ => None,
    });
    let target_unit = settings.get("target.unit").and_then(|name| match name {
        SettingValue::Str(s) => Some(s.clone()),
        _ => None,
    });
    let self_path = settings.get("selfpath").and_then(|dir| match dir {
        SettingValue::Str(s) => Some(resolve_relative_path(&PathBuf::from(s), &default_config_dir)),
        _ => None,
    });
    let unit_dirs = settings.get("unit.dirs").map(|dir| match dir {
        SettingValue::Str(s) => vec![resolve_relative_path(&PathBuf::from(s), &default_config_dir)],
        SettingValue::Array(arr) => arr
            .iter()
            .filter_map(|el| match el {
                SettingValue::Str(s) => Some(resolve_relative_path(&PathBuf::from(s), &default_config_dir)),
                _ => None,
            })
            .filter(|path| {
                let ok = path.exists();
                if !ok {
                    // Sebelumnya baris ini diam-diam membuang direktori yang
                    // tidak ada tanpa jejak apa pun -- operator bisa salah
                    // konfigurasi unit_dirs dan tidak pernah tahu kenapa unit-nya
                    // tidak termuat. Sekarang minimal ada peringatan di stderr.
                    crate::ui::warning(format!(
                        "lksystem: unit_dirs entry {} tidak ditemukan, dilewati",
                        path.display()
                    ));
                }
                ok
            })
            .collect(),
        _ => Vec::new(),
    });
    let sqlite_db_path = settings.get("sqlite.db").and_then(|dir| match dir {
        SettingValue::Str(s) => Some(resolve_relative_path(&PathBuf::from(s), &default_config_dir)),
        _ => None,
    });
    let default_unit_dirs = {
        let candidate = default_config_dir.join("unitfiles");
        if candidate.exists() {
            vec![candidate]
        } else {
            vec![PathBuf::from("./unitfiles")]
        }
    };
    let default_notifications_dir = if default_config_dir == PathBuf::from("/etc/lksystem") {
        PathBuf::from("/run/lksystem/notifications")
    } else {
        PathBuf::from("./notifications")
    };
    let default_sqlite_db_path = if default_config_dir == PathBuf::from("/etc/lksystem") {
        PathBuf::from("/run/lksystem/db.sqlite")
    } else {
        default_config_dir.join("db.sqlite")
    };
    let config = Config {
        unit_dirs: unit_dirs.unwrap_or_else(|| default_unit_dirs),
        target_unit: target_unit.unwrap_or_else(|| "default.target".to_owned()),
        notification_sockets_dir: notification_sockets_dir
            .unwrap_or_else(|| default_notifications_dir),
        self_path: self_path.unwrap_or_else(|| {
            std::env::current_exe()
                .expect("Could not get own executable name and it was not configured explicitly")
        }),
        sqlite_db_path: sqlite_db_path.unwrap_or(default_sqlite_db_path),
    };
    let mut config_load_error = None;
    let config_files_found = [lua_conf.is_some(), json_conf.is_some()]
        .iter()
        .filter(|&&present| present)
        .count();
    if config_files_found > 1 {
        config_load_error = Some(Err("Found multiple config file formats in the config directory".into()));
    }
    let conf = if let Some(error) = config_load_error {
        error
    } else if let Some(lua_conf) = lua_conf {
        lua_conf.map(|_| config)
    } else if let Some(json_conf) = json_conf {
        json_conf.map(|_| config)
    } else if config_path.is_some() {
        Err("No config file was loaded".into())
    } else {
        Ok(config)
    };
    let config = conf.and_then(|config| {
        initialize_sqlite_db(&config.sqlite_db_path).map(|_| config)
    });
    (
        LoggingConfig {
            log_dir: log_dir
                .unwrap_or_else(|| PathBuf::from("./logs")),
            log_to_disk: log_to_disk.unwrap_or(false),
            log_to_stdout: log_to_stdout.unwrap_or(true),
        },
        config,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_lua_config() {
        let temp_dir = std::env::temp_dir().join("lksystem_config_lua_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("test_units")).unwrap();

        let lua_config = r#"
            return {
                logging_dir = "./logs",
                log_to_stdout = true,
                log_to_disk = false,
                notifications_dir = "./notifications",
                unit_dirs = {"./test_units"},
                target_unit = "default.target",
                selfpath = "./lksystem",
                sqlite_db = "./lksystem.db"
            }
        "#;
        fs::write(temp_dir.join("config.lua"), lua_config).unwrap();

        let (log_conf, config_result) = load_config(&Some(temp_dir.clone()));
        assert!(config_result.is_ok(), "config load failed: {:?}", config_result);
        let config = config_result.unwrap();

        assert_eq!(config.target_unit, "default.target");
        assert_eq!(config.unit_dirs, vec![temp_dir.join("test_units")]);
        assert_eq!(config.notification_sockets_dir, temp_dir.join("notifications"));
        assert_eq!(config.self_path, temp_dir.join("lksystem"));
        assert_eq!(config.sqlite_db_path, temp_dir.join("lksystem.db"));
        assert_eq!(log_conf.log_dir, temp_dir.join("logs"));
        assert!(log_conf.log_to_stdout);
        assert!(!log_conf.log_to_disk);
    }
}
