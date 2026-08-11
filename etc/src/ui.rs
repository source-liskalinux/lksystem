//! Small terminal UI used by the Rust init stages.
//!
//! The native C supervisor has an equivalent implementation in `src/ui.c`.
use std::fs;
use std::io::{self, IsTerminal, Write};

const RESET: &str = "\x1b[0m";
const CYAN: &str = "\x1b[1;36m";
const GREEN: &str = "\x1b[1;32m";
const YELLOW: &str = "\x1b[1;33m";
const RED: &str = "\x1b[1;31m";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

fn emit(level: Level, message: impl std::fmt::Display) {
    let mut stderr = io::stderr().lock();
    let (prefix, colour, colour_message) = match level {
        Level::Info => ("[i]", CYAN, false),
        Level::Success => ("[✓]", GREEN, true),
        Level::Warning => ("[!]", YELLOW, true),
        Level::Error => ("[✗]", RED, true),
    };

    if stderr.is_terminal() {
        let _ = write!(stderr, "{colour}{prefix}{RESET} ");
        if colour_message {
            let _ = writeln!(stderr, "{colour}{message}{RESET}");
        } else {
            let _ = writeln!(stderr, "{message}");
        }
    } else {
        let _ = writeln!(stderr, "{prefix} {message}");
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

// Shows the boot banner using the distribution name supplied by os-release
pub fn welcome() {
    let name = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|os_release| os_name(&os_release))
        .unwrap_or_else(|| "Linux".to_owned());
    let mut stderr = io::stderr().lock();
    if stderr.is_terminal() {
        let _ = writeln!(stderr, "{CYAN}::: [ Welcome To {name} ] :::{RESET}");
    } else {
        let _ = writeln!(stderr, "::: [ Welcome To {name} ] :::");
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

#[cfg(test)]
mod tests {
    use super::os_name;
    #[test]
    fn prefers_pretty_name_from_os_release() {
        assert_eq!(
            os_name("NAME=Linux\nPRETTY_NAME=\"Lksystem OS\"\n"),
            Some("Lksystem OS".to_owned())
        );
    }
    #[test]
    fn falls_back_to_name_and_unescapes_quotes() {
        assert_eq!(
            os_name(r#"NAME="Example \"Linux\"""#),
            Some("Example \"Linux\"".to_owned())
        );
    }
}
