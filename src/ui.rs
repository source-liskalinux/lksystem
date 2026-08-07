use colored::*;

pub fn log(message: impl std::fmt::Display) {
    println!("{} {}", "[ i ]".bright_cyan(), message);
}

pub fn success(message: impl std::fmt::Display) {
    println!("{} {}", "[ ✓ ]".bright_green(), message.to_string().bright_green());
}

pub fn warning(message: impl std::fmt::Display) {
    println!("{} {}", "[ ! ]".bright_yellow(), message.to_string().bright_yellow());
}

pub fn error(message: impl std::fmt::Display) {
    println!("{} {}", "[ ✗ ]".bright_red(), message.to_string().bright_red());
}

pub fn write(message: impl std::fmt::Display) {
    print!("{}", message);
}

pub fn write_line(message: impl std::fmt::Display) {
    println!("{}", message);
}

pub fn write_error(message: impl std::fmt::Display) {
    eprintln!("{}", message.to_string().bright_red());
}