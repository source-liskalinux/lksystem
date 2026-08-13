use lksystem::ui;
use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::process::Command;

type UidT = u32;
type GidT = u32;
type RlimT = u64;

#[repr(C)]
struct Passwd {
    pw_name: *mut c_char,
    pw_passwd: *mut c_char,
    pw_uid: UidT,
    pw_gid: GidT,
    pw_gecos: *mut c_char,
    pw_dir: *mut c_char,
    pw_shell: *mut c_char,
}

#[repr(C)]
struct Group {
    gr_name: *mut c_char,
    gr_passwd: *mut c_char,
    gr_gid: GidT,
    gr_mem: *mut *mut c_char,
}

#[repr(C)]
struct RLimit {
    rlim_cur: RlimT,
    rlim_max: RlimT,
}

const RLIMIT_CPU: c_int = 0;
const RLIMIT_FSIZE: c_int = 1;
const RLIMIT_DATA: c_int = 2;
const RLIMIT_STACK: c_int = 3;
const RLIMIT_CORE: c_int = 4;
const RLIMIT_NPROC: c_int = 6;
const RLIMIT_NOFILE: c_int = 7;
const RLIMIT_AS: c_int = 9;
const LOCK_EX: c_int = 2;

unsafe extern "C" {
    fn getpwnam(name: *const c_char) -> *mut Passwd;
    fn getgrnam(name: *const c_char) -> *mut Group;
    fn setuid(uid: UidT) -> c_int;
    fn setgid(gid: GidT) -> c_int;
    fn setgroups(count: usize, groups: *const GidT) -> c_int;
    fn initgroups(user: *const c_char, group: GidT) -> c_int;
    fn chroot(path: *const c_char) -> c_int;
    fn chdir(path: *const c_char) -> c_int;
    fn setsid() -> c_int;
    fn setrlimit(resource: c_int, limit: *const RLimit) -> c_int;
    fn flock(fd: c_int, operation: c_int) -> c_int;
    fn nice(inc: c_int) -> c_int;
    fn __errno_location() -> *mut c_int;
    fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int;
}

const F_GETFD: c_int = 1;
const F_SETFD: c_int = 2;
const FD_CLOEXEC: c_int = 1;

fn clear_errno() {
    unsafe { *__errno_location() = 0 };
}

fn last_errno() -> c_int {
    unsafe { *__errno_location() }
}

fn die(message: impl std::fmt::Display) -> ! {
    ui::error(format!("{message}"));
    std::process::exit(111);
}

fn cstring(value: &str) -> CString {
    CString::new(value).unwrap_or_else(|_| die("Argument contains a NUL byte!"))
}

struct Options {
    verbose: bool,
    argv0: Option<String>,
    envdir: Option<String>,
    root: Option<String>,
    user: Option<String>,
    user_is_env_only: bool,
    nice_incr: Option<i32>,
    new_session: bool,
    close_stdin: bool,
    close_stdout: bool,
    close_stderr: bool,
    lock: Option<(String, bool)>,
    limits: Vec<(c_int, RlimT)>,
}

impl Options {
    fn new() -> Self {
        Self {
            verbose: false,
            argv0: None,
            envdir: None,
            root: None,
            user: None,
            user_is_env_only: false,
            nice_incr: None,
            new_session: false,
            close_stdin: false,
            close_stdout: false,
            close_stderr: false,
            lock: None,
            limits: Vec::new(),
        }
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: chpst [-v] [-u [:]user[:group[:group...]]] [-U user[:group[:group...]]]\n\
         [-b argv0] [-e envdir] [-/ root] [-n incr] [-P]\n\
         [-0] [-1] [-2] [-l|-L lockfile]\n\
         [-m bytes] [-d bytes] [-o num] [-p num] [-f bytes] [-c bytes]\n\
         [-t seconds] [-s bytes (extension, RLIMIT_STACK)]\n\
         prog [args...]"
    );
    std::process::exit(100);
}

fn parse_bytes(value: &str) -> RlimT {
    value.parse().unwrap_or_else(|_| usage())
}

fn required(arguments: &mut impl Iterator<Item = String>, flag: &str) -> String {
    arguments
        .next()
        .unwrap_or_else(|| die(format!("{flag} requires an argument!")))
}

fn parse_args() -> (Options, Vec<String>) {
    let mut options = Options::new();
    let mut arguments = std::env::args().skip(1).peekable();
    while let Some(argument) = arguments.peek().cloned() {
        if argument == "--" {
            arguments.next();
            break;
        }
        if !argument.starts_with('-') || argument == "-" {
            break;
        }
        arguments.next();
        match argument.as_str() {
            "-v" => options.verbose = true,
            "-P" => options.new_session = true,
            "-0" => options.close_stdin = true,
            "-1" => options.close_stdout = true,
            "-2" => options.close_stderr = true,
            "-b" => options.argv0 = Some(required(&mut arguments, "-b")),
            "-e" => options.envdir = Some(required(&mut arguments, "-e")),
            "-/" => options.root = Some(required(&mut arguments, "-/")),
            "-n" => {
                let value = required(&mut arguments, "-n");
                options.nice_incr = Some(value.parse().unwrap_or_else(|_| usage()));
            }
            "-u" => {
                options.user = Some(required(&mut arguments, "-u"));
                options.user_is_env_only = false;
            }
            "-U" => {
                options.user = Some(required(&mut arguments, "-U"));
                options.user_is_env_only = true;
            }
            "-l" => options.lock = Some((required(&mut arguments, "-l"), true)),
            "-L" => options.lock = Some((required(&mut arguments, "-L"), false)),
            "-m" => options
                .limits
                .push((RLIMIT_AS, parse_bytes(&required(&mut arguments, "-m")))),
            "-d" => options
                .limits
                .push((RLIMIT_DATA, parse_bytes(&required(&mut arguments, "-d")))),
            "-s" => options
                .limits
                .push((RLIMIT_STACK, parse_bytes(&required(&mut arguments, "-s")))),
            "-o" => options
                .limits
                .push((RLIMIT_NOFILE, parse_bytes(&required(&mut arguments, "-o")))),
            "-p" => options
                .limits
                .push((RLIMIT_NPROC, parse_bytes(&required(&mut arguments, "-p")))),
            "-f" => options
                .limits
                .push((RLIMIT_FSIZE, parse_bytes(&required(&mut arguments, "-f")))),
            "-c" => options
                .limits
                .push((RLIMIT_CORE, parse_bytes(&required(&mut arguments, "-c")))),
            "-t" => options
                .limits
                .push((RLIMIT_CPU, parse_bytes(&required(&mut arguments, "-t")))),
            _ => usage(),
        }
    }
    let program: Vec<String> = arguments.collect();
    if program.is_empty() {
        usage();
    }
    (options, program)
}

fn lookup_user(name: &str) -> (UidT, GidT) {
    let cname = cstring(name);
    let entry = unsafe { getpwnam(cname.as_ptr()) };
    if entry.is_null() {
        die(format!("Unknown user: {name}."));
    }
    unsafe { ((*entry).pw_uid, (*entry).pw_gid) }
}

fn lookup_group(name: &str) -> GidT {
    if let Ok(gid) = name.parse::<GidT>() {
        return gid;
    }
    let cname = cstring(name);
    let entry = unsafe { getgrnam(cname.as_ptr()) };
    if entry.is_null() {
        die(format!("Unknown group: {name}."));
    }
    unsafe { (*entry).gr_gid }
}

fn apply_privileges(options: &Options) {
    let Some(spec) = &options.user else { return };
    let (by_id, spec) = match spec.strip_prefix(':') {
        Some(rest) => (true, rest),
        None => (false, spec.as_str()),
    };
    let mut parts = spec.split(':');
    let user_field = parts.next().unwrap_or("");
    let group_fields: Vec<&str> = parts.collect();
    let (uid, default_gid, user_name) = if by_id {
        let uid: UidT = user_field
            .parse()
            .unwrap_or_else(|_| die(format!("Invalid numeric uid: {user_field}.")));
        (uid, uid, None)
    } else {
        let (uid, gid) = lookup_user(user_field);
        (uid, gid, Some(user_field.to_owned()))
    };
    let groups: Vec<GidT> = group_fields
        .iter()
        .map(|field| {
            if by_id {
                field
                    .parse()
                    .unwrap_or_else(|_| die(format!("Invalid numeric gid: {field}.")))
            } else {
                lookup_group(field)
            }
        })
        .collect();
    if options.user_is_env_only {
        let gid = groups.first().copied().unwrap_or(default_gid);
        unsafe {
            std::env::set_var("UID", uid.to_string());
            std::env::set_var("GID", gid.to_string());
        }
        return;
    }
    if groups.is_empty() {
        match &user_name {
            Some(name) => {
                let cname = cstring(name);
                if unsafe { initgroups(cname.as_ptr(), default_gid) } == -1 {
                    die(format!(
                        "Cannot set supplementary groups for {name}! Err: {}.",
                        io::Error::last_os_error()
                    ));
                }
            }
            None => {
                if unsafe { setgroups(0, std::ptr::null()) } == -1 {
                    die(format!("Cannot clear supplementary groups! Err: {}.", io::Error::last_os_error()));
                }
            }
        }
        if unsafe { setgid(default_gid) } == -1 {
            die(format!("Cannot setgid({default_gid})! Err: {}.", io::Error::last_os_error()));
        }
    } else {
        if unsafe { setgroups(groups.len(), groups.as_ptr()) } == -1 {
            die(format!("Cannot setgroups! Err: {}.", io::Error::last_os_error()));
        }
        let primary = groups[0];
        if unsafe { setgid(primary) } == -1 {
            die(format!("Cannot setgid({primary})! Err: {}.", io::Error::last_os_error()));
        }
    }
    if unsafe { setuid(uid) } == -1 {
        die(format!("Cannot setuid({uid})! Err: {}.", io::Error::last_os_error()));
    }
}

fn apply_envdir(directory: &str) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| die(format!("Cannot read envdir {directory}! Err: {error}.")));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| die(format!("Cannot read envdir entry! Err: {error}.")));
        if !entry.file_type().map(|kind| kind.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if name.is_empty() || name.contains('=') {
            continue;
        }
        let bytes = fs::read(entry.path())
            .unwrap_or_else(|error| die(format!("Cannot read envdir/{name}! Err: {error}.")));
        if bytes.is_empty() {
            unsafe { std::env::remove_var(&name) };
            continue;
        }
        let first_line = bytes.split(|&byte| byte == b'\n').next().unwrap_or(&[]);
        let mut value: Vec<u8> = first_line
            .iter()
            .map(|&byte| if byte == 0 { b'\n' } else { byte })
            .collect();
        while matches!(value.last(), Some(b' ' | b'\t')) {
            value.pop();
        }
        let value = String::from_utf8_lossy(&value).into_owned();
        unsafe { std::env::set_var(&name, value) };
    }
}

fn apply_root(root: &str) {
    let croot = cstring(root);
    if unsafe { chroot(croot.as_ptr()) } == -1 {
        die(format!("Cannot chroot to {root}! Err: {}.", io::Error::last_os_error()));
    }
    let cslash = cstring("/");
    if unsafe { chdir(cslash.as_ptr()) } == -1 {
        die(format!("Cannot chdir to root after chroot! Err: {}.", io::Error::last_os_error()));
    }
}

fn apply_nice(incr: i32) {
    clear_errno();
    let result = unsafe { nice(incr as c_int) };
    if result == -1 && last_errno() != 0 {
        die(format!("Cannot nice({incr})! Err: {}.", io::Error::last_os_error()));
    }
}

fn apply_limits(limits: &[(c_int, RlimT)]) {
    for &(resource, value) in limits {
        let limit = RLimit {
            rlim_cur: value,
            rlim_max: value,
        };
        if unsafe { setrlimit(resource, &limit) } == -1 {
            die(format!(
                "Cannot set resource limit ({resource}) to {value}! Err: {}.",
                io::Error::last_os_error()
            ));
        }
    }
}

fn apply_lock(path: &str, wait: bool) {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .unwrap_or_else(|error| die(format!("Cannot open lockfile {path}! Err: {error}.")));
    let fd = file.as_raw_fd();
    let operation = if wait { LOCK_EX } else { LOCK_EX | 4 /* LOCK_NB */ };
    if unsafe { flock(fd, operation) } == -1 {
        die(format!("Cannot lock {path}! Err: {}.", io::Error::last_os_error()));
    }
    let flags = unsafe { fcntl(fd, F_GETFD, 0) };
    if flags != -1 {
        unsafe { fcntl(fd, F_SETFD, flags & !FD_CLOEXEC) };
    }
    std::mem::forget(file);
}

fn apply_session(options: &Options) {
    if options.new_session && unsafe { setsid() } == -1 {
        die(format!("Cannot setsid! Err: {}.", io::Error::last_os_error()));
    }
}

fn main() {
    let (options, program) = parse_args();
    if let Some((path, wait)) = &options.lock {
        apply_lock(path, *wait);
    }
    if let Some(root) = &options.root {
        apply_root(root);
    }
    if let Some(envdir) = &options.envdir {
        apply_envdir(envdir);
    }
    apply_privileges(&options);
    if let Some(incr) = options.nice_incr {
        apply_nice(incr);
    }
    apply_limits(&options.limits);
    apply_session(&options);
    if options.close_stdin {
        let _ = fs::File::open("/dev/null").map(|null| unsafe {
            libc_dup2(null.as_raw_fd(), 0);
        });
    }
    if options.close_stdout {
        let _ = OpenOptions::new().write(true).open("/dev/null").map(|null| unsafe {
            libc_dup2(null.as_raw_fd(), 1);
        });
    }
    if options.close_stderr {
        let _ = OpenOptions::new().write(true).open("/dev/null").map(|null| unsafe {
            libc_dup2(null.as_raw_fd(), 2);
        });
    }
    let program_path = Path::new(&program[0]);
    let mut command = Command::new(program_path);
    command.args(&program[1..]);
    if let Some(argv0) = &options.argv0 {
        std::os::unix::process::CommandExt::arg0(&mut command, argv0);
    }
    if options.verbose {
        ui::log(format!("Running {}....", program.join(" ")));
    }
    use std::os::unix::process::CommandExt;
    let error = command.exec();
    die(format!("Cannot exec {}! Err: {error}.", program[0]));
}

unsafe extern "C" {
    #[link_name = "dup2"]
    fn libc_dup2(oldfd: c_int, newfd: c_int) -> c_int;
}
