use lksystem_ui::{linux, ui};
use std::env;
use std::io;
use std::process::Command;

fn main() -> io::Result<()> {
    ui::log("Entering lksystem stage 2....");
    let service_dir =
        env::var("LKSYSTEM_SERVICE_DIR").unwrap_or_else(|_| linux::SERVICE_DIR.to_owned());
    let lksysdir = env::var("LKSYSTEM_lksysDIR").unwrap_or_else(|_| "lksysdir".to_owned());
    match linux::activate_virtual_terminal(linux::DEFAULT_TTY) {
        Ok(true) => ui::success("Default login console switched to tty1."),
        Ok(false) => ui::log("No virtual console available; keeping the current console."),
        Err(error) => ui::warning(format!(
            "Cannot switch default login console to tty1: {error}"
        )),
    }
    ui::log(format!("Starting lksysdir for {service_dir}...."));
    let mut command = Command::new(lksysdir);
    command.args(["-P", &service_dir]).env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    ui::success("Lksystem stage 2 handing off to lksysdir.");
    Err(std::os::unix::process::CommandExt::exec(&mut command))
}
