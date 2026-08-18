use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SERVICES_DIR: &str = "/etc/lksystem/services";
pub const STATUS_LEN: usize = 20;
pub const STATE_DOWN: u8 = 0;
pub const STATE_RUN: u8 = 1;
pub const STATE_FINISH: u8 = 2;
pub const WANT_UP: u8 = b'u';
pub const WANT_DOWN: u8 = b'd';
pub const SIGTERM: i32 = 15;
pub const SIGHUP: i32 = 1;
pub const SIGKILL: i32 = 9;
pub const SIGSTOP: i32 = 19;
pub const SIGCONT: i32 = 18;
pub const SIGALRM: i32 = 14;
pub const SIGINT: i32 = 2;
pub const SIGQUIT: i32 = 3;
pub const SIGUSR1: i32 = 10;
pub const SIGUSR2: i32 = 12;

const O_NONBLOCK: i32 = 0o4000;
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const EEXIST: i32 = 17;

unsafe extern "C" {
    fn mkfifo(path: *const i8, mode: u32) -> i32;
    fn flock(fd: i32, operation: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(signal: i32, handler: extern "C" fn(i32)) -> usize;
}

pub static TERMINATE: AtomicBool = AtomicBool::new(false);
pub static RELOAD: AtomicBool = AtomicBool::new(false);

extern "C" fn termination_handler(_: i32) {
    TERMINATE.store(true, Ordering::Relaxed);
}

extern "C" fn reload_handler(_: i32) {
    RELOAD.store(true, Ordering::Relaxed);
}

pub fn install_signal_handlers() {
    unsafe {
        signal(SIGTERM, termination_handler);
        signal(SIGHUP, reload_handler);
    }
}

pub fn take_terminate() -> bool {
    TERMINATE.swap(false, Ordering::Relaxed)
}

pub fn take_reload() -> bool {
    RELOAD.swap(false, Ordering::Relaxed)
}

pub fn make_fifo(path: &Path) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    let result = unsafe { mkfifo(path.as_ptr(), 0o600) };
    if result == 0 || io::Error::last_os_error().raw_os_error() == Some(EEXIST) {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn open_fifo(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
}

pub fn open_fifo_writer(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
}

pub fn read_available(file: &mut File) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 256];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(length) => bytes.extend_from_slice(&buffer[..length]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

pub fn lock_supervise(directory: &Path) -> io::Result<File> {
    fs::create_dir_all(directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(directory.join("lock"))?;
    if unsafe { flock(lock.as_raw_fd(), LOCK_EX | LOCK_NB) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(lock)
}

pub fn tai_now() -> [u8; 12] {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let tai = 4_611_686_018_427_387_914_u64.saturating_add(elapsed.as_secs());
    let mut bytes = [0_u8; 12];
    bytes[..8].copy_from_slice(&tai.to_be_bytes());
    bytes[8..12].copy_from_slice(&elapsed.subsec_nanos().saturating_mul(1_000).to_be_bytes());
    bytes
}

#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub started: [u8; 12],
    pub pid: u32,
    pub paused: bool,
    pub want: u8,
    pub got_term: bool,
    pub state: u8,
}

impl Status {
    pub fn down() -> Self {
        Self {
            started: tai_now(),
            pid: 0,
            paused: false,
            want: WANT_UP,
            got_term: false,
            state: STATE_DOWN,
        }
    }

    pub fn encode(self) -> [u8; STATUS_LEN] {
        let mut bytes = [0_u8; STATUS_LEN];
        bytes[..12].copy_from_slice(&self.started);
        bytes[12..16].copy_from_slice(&self.pid.to_le_bytes());
        bytes[16] = self.paused.into();
        bytes[17] = self.want;
        bytes[18] = self.got_term.into();
        bytes[19] = self.state;
        bytes
    }

    pub fn decode(bytes: [u8; STATUS_LEN]) -> Self {
        let mut started = [0_u8; 12];
        started.copy_from_slice(&bytes[..12]);
        Self {
            started,
            pid: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            paused: bytes[16] != 0,
            want: bytes[17],
            got_term: bytes[18] != 0,
            state: bytes[19],
        }
    }
}

pub fn write_status(service: &Path, status: Status) -> io::Result<()> {
    let supervise = service.join("supervise");
    let status_new = supervise.join("status.new");
    fs::write(&status_new, status.encode())?;
    fs::rename(status_new, supervise.join("status"))?;
    let description = match status.state {
        STATE_RUN => "run",
        STATE_FINISH => "finish",
        _ => "down",
    };
    let mut stat = description.to_owned();
    if status.paused {
        stat.push_str(", paused");
    }
    if status.got_term {
        stat.push_str(", got TERM");
    }
    if status.state != STATE_DOWN && status.want != WANT_UP {
        stat.push_str(", want down");
    }
    stat.push('\n');
    fs::write(supervise.join("stat.new"), stat)?;
    fs::rename(supervise.join("stat.new"), supervise.join("stat"))?;
    fs::write(
        supervise.join("pid.new"),
        if status.pid == 0 {
            String::new()
        } else {
            format!("{}\n", status.pid)
        },
    )?;
    fs::rename(supervise.join("pid.new"), supervise.join("pid"))
}

pub fn read_status(service: &Path, log: bool) -> io::Result<Status> {
    let path = if log {
        service.join("log/supervise/status")
    } else {
        service.join("supervise/status")
    };
    let bytes = fs::read(path)?;
    let bytes: [u8; STATUS_LEN] = bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid supervise/status"))?;
    Ok(Status::decode(bytes))
}

pub fn send_signal(pid: u32, signal: i32) -> io::Result<()> {
    if pid == 0 || unsafe { kill(pid as i32, signal) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn service_path(name: &str) -> PathBuf {
    let path = PathBuf::from(name);
    if path.is_absolute() || name.starts_with('.') {
        path
    } else {
        std::env::var_os("LKSYSTEM_SERVICES_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(SERVICES_DIR))
            .join(name)
    }
}

/// Well-known file PID 1 reads right before it calls reboot(2), so a
/// system-wide "reboot"/"poweroff"/"halt" request can be communicated at
/// runtime. A plain `kill -TERM 1` (or any other trigger that doesn't write
/// this file first) falls back to lksystem.rs's own LKSYSTEM_REBOOT env-var
/// default, so existing behavior is unaffected.
pub const REBOOT_CMD_FILE: &str = "/run/lksystem/reboot-cmd";

/// Requests a system-wide reboot/poweroff/halt: records the desired action
/// for PID 1 to pick up, then signals it with SIGTERM. `action` should be
/// one of "reboot", "poweroff", or "halt".
pub fn request_system_shutdown(action: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(REBOOT_CMD_FILE).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(REBOOT_CMD_FILE, action)?;
    send_signal(1, SIGTERM)
}
