use colored::{Color, Colorize};
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
        let _ = writeln!(stderr, "{icon} {}", message.to_string().color(colour).bold());
    } else {
        let _ = writeln!(stderr, "{icon} {message}");
    }
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
pub fn line(message) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{message}");
}