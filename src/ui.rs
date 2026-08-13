use colored::{Color, Colorize};
use std::fs;
use std::io::{self, IsTerminal, Write};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

fn sync_color_override() {
    let enable = io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    colored::control::set_override(enable);
}

fn emit(level: Level, message: impl std::fmt::Display) {
    sync_color_override();
    let mut stderr = io::stderr().lock();
    let (icon, colour, colour_message) = match level {
        Level::Info => ("[i]", Color::BrightCyan, false),
        Level::Success => ("[✓]", Color::BrightGreen, true),
        Level::Warning => ("[!]", Color::BrightYellow, true),
        Level::Error => ("[✗]", Color::BrightRed, true),
    };
    let icon = icon.color(colour).bold();
    if colour_message {
        let _ = writeln!(stderr, "{icon} {}", message.to_string().color(colour));
    } else {
        let _ = writeln!(stderr, "{icon} {message}");
    }
}

fn os_name(os_release: &str) -> Option<String> {
    for key in ["PRETTY_NAME", "NAME"] {
        if let Some(value) = os_release
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
        {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            if !value.is_empty() {
                return Some(value.replace(r#"\""#, "\"").replace(r#"\\"#, "\\"));
            }
        }
    }
    None
}

pub fn welcome() {
    let name = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|os_release| os_name(&os_release))
        .unwrap_or_else(|| "Linux".to_owned());
    sync_color_override();
    let mut stderr = io::stderr().lock();
    let banner = format!("            ::: [ Welcome To {name}! ] :::");
    let _ = writeln!(stderr, "");
    let _ = writeln!(stderr, "");
    let _ = writeln!(stderr, "{}", banner.color(Color::BrightCyan).bold());
    let _ = writeln!(stderr, "");
    let _ = writeln!(stderr, "");
}

pub fn log(message: impl std::fmt::Display) {
    emit(Level::Info, message);
}
pub fn success(message: impl std::fmt::Display) {
    emit(Level::Success, message);
}
pub fn warning(message: impl std::fmt::Display) {
    emit(Level::Warning, message);
}
pub fn error(message: impl std::fmt::Display) {
    emit(Level::Error, message);
}

