//! Linux-only process helpers used by the lksystem stage programs.

#![allow(dead_code)]

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_ulong};

pub const CONFIG_DIR: &str = "/etc/lksystem";
pub const SERVICE_DIR: &str = "/etc/lksystem/service";
pub const DEFAULT_TTY: u32 = 1;

const ENOTTY: i32 = 25;
const ENODEV: i32 = 19;
const ENXIO: i32 = 6;
const VT_ACTIVATE: c_ulong = 0x5606;
const VT_WAITACTIVE: c_ulong = 0x5607;

unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

fn is_virtual_terminal_unavailable(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ENOTTY) | Some(ENODEV) | Some(ENXIO)
    )
}

fn ioctl_result(result: c_int) -> io::Result<()> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn activate_virtual_terminal(tty: u32) -> io::Result<bool> {
    if tty == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "virtual terminal numbers start at 1",
        ));
    }
    let console = match OpenOptions::new().read(true).write(true).open("/dev/tty0") {
        Ok(console) => console,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) if is_virtual_terminal_unavailable(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let tty = tty as c_int;
    unsafe {
        if let Err(error) = ioctl_result(ioctl(console.as_raw_fd(), VT_ACTIVATE, tty)) {
            if is_virtual_terminal_unavailable(&error) {
                return Ok(false);
            }
            return Err(error);
        }
        if let Err(error) = ioctl_result(ioctl(console.as_raw_fd(), VT_WAITACTIVE, tty)) {
            if is_virtual_terminal_unavailable(&error) {
                return Ok(false);
            }
            return Err(error);
        }
    }
    Ok(true)
}
