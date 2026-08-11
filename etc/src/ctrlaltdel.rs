use lksystem_ui::{linux, ui};
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() -> io::Result<()> {
    ui::warning("Ctrl + Alt + Del received! Shutdown has been initialized.");
    fs::create_dir_all(linux::CONFIG_DIR)?;
    let stopit = format!("{}/stopit", linux::CONFIG_DIR);
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&stopit)?;
    fs::set_permissions(&stopit, fs::Permissions::from_mode(0o100))?;
    let message = "System is shutting down in 10 seconds....";
    if let Ok(mut wall) = Command::new("wall").stdin(Stdio::piped()).spawn() {
        if let Some(mut stdin) = wall.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(message.as_bytes());
        }
        let _ = wall.wait();
    }
    thread::sleep(Duration::from_secs(10));
    Ok(())
}
