use lksystem::core::{
    install_signal_handlers, lock_supervise, make_fifo, open_fifo, read_available, send_signal,
    tai_now, take_terminate, write_status, Status, STATE_DOWN, STATE_FINISH, STATE_RUN, WANT_DOWN,
    WANT_UP,
};
use std::env;
use std::fs::File;
use std::io;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn pipe(fds: *mut i32) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Want {
    Up,
    Down,
    Exit,
}

impl Want {
    fn status_byte(self) -> u8 {
        match self {
            Self::Up => WANT_UP,
            Self::Down | Self::Exit => WANT_DOWN,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Down,
    Run,
    Finish,
}

impl Phase {
    fn status_byte(self) -> u8 {
        match self {
            Self::Down => STATE_DOWN,
            Self::Run => STATE_RUN,
            Self::Finish => STATE_FINISH,
        }
    }
}

struct Process {
    child: Child,
    phase: Phase,
    started: [u8; 12],
    got_term: bool,
    paused: bool,
}

struct Supervisor {
    service: PathBuf,
    want: Want,
    process: Option<Process>,
    control: File,
    _lock: File,
    logger: Option<Child>,
    retry_at: Instant,
}

fn usage() -> ! {
    eprintln!("usage: lksys dir");
    std::process::exit(1);
}

fn pipe_files() -> io::Result<(File, File)> {
    let mut fds = [-1_i32; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) })
}

fn spawn_script(
    service: &Path,
    script: &str,
    args: &[String],
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
) -> io::Result<Child> {
    let path = service.join(script);
    let mut command = Command::new(&path);
    command.current_dir(service).args(args);
    if let Some(stdin) = stdin {
        command.stdin(stdin);
    }
    if let Some(stdout) = stdout {
        command.stdout(stdout);
    }
    command.spawn()
}

fn status_code(status: ExitStatus) -> (String, String) {
    use std::os::unix::process::ExitStatusExt;
    let code = status.code().unwrap_or(-1).to_string();
    let signal = status.signal().unwrap_or(0).to_string();
    (code, signal)
}

impl Supervisor {
    fn new(service: PathBuf) -> io::Result<Self> {
        let supervise = service.join("supervise");
        let lock = lock_supervise(&supervise)?;
        make_fifo(&supervise.join("control"))?;
        make_fifo(&supervise.join("ok"))?;
        let control = open_fifo(&supervise.join("control"))?;
        let _ok = open_fifo(&supervise.join("ok"))?;
        let want = if service.join("down").exists() {
            Want::Down
        } else {
            Want::Up
        };
        let instance = Self {
            service,
            want,
            process: None,
            control,
            _lock: lock,
            logger: None,
            retry_at: Instant::now(),
        };
        instance.publish()?;
        Ok(instance)
    }

    fn publish(&self) -> io::Result<()> {
        let (phase, pid, started, paused, got_term) = match &self.process {
            Some(process) => (
                process.phase,
                process.child.id(),
                process.started,
                process.paused,
                process.got_term,
            ),
            None => (Phase::Down, 0, tai_now(), false, false),
        };
        write_status(
            &self.service,
            Status {
                started,
                pid,
                paused,
                want: self.want.status_byte(),
                got_term,
                state: phase.status_byte(),
            },
        )
    }

    fn start_logger(&mut self) -> io::Result<Option<Stdio>> {
        let log = self.service.join("log");
        if !log.join("run").is_file() {
            return Ok(None);
        }
        let (reader, writer) = pipe_files()?;
        let logger = spawn_script(&log, "run", &[], Some(Stdio::from(reader)), None)?;
        self.logger = Some(logger);
        Ok(Some(unsafe { Stdio::from_raw_fd(writer.into_raw_fd()) }))
    }

    fn start_run(&mut self) -> io::Result<()> {
        let stdout = self.start_logger()?;
        let child = spawn_script(&self.service, "run", &[], None, stdout)?;
        self.process = Some(Process {
            child,
            phase: Phase::Run,
            started: tai_now(),
            got_term: false,
            paused: false,
        });
        self.publish()
    }

    fn start_finish(&mut self, status: ExitStatus) -> io::Result<()> {
        let finish = self.service.join("finish");
        if !finish.is_file() {
            self.process = None;
            self.retry_at = Instant::now() + Duration::from_secs(1);
            return self.publish();
        }
        let (code, signal) = status_code(status);
        let child = spawn_script(&self.service, "finish", &[code, signal], None, None)?;
        self.process = Some(Process {
            child,
            phase: Phase::Finish,
            started: tai_now(),
            got_term: false,
            paused: false,
        });
        self.publish()
    }

    fn stop(&mut self, signal: i32) {
        if let Some(process) = &mut self.process {
            let _ = send_signal(process.child.id(), signal);
            if signal == lksystem::core::SIGTERM {
                process.got_term = true;
                let _ = self.publish();
            }
        }
    }

    fn apply_control(&mut self, byte: u8) {
        match byte {
            b'd' => {
                self.want = Want::Down;
                self.stop(lksystem::core::SIGTERM);
            }
            b'u' => self.want = Want::Up,
            b'x' => {
                self.want = Want::Exit;
                self.stop(lksystem::core::SIGTERM);
            }
            b't' => self.stop(lksystem::core::SIGTERM),
            b'k' => self.stop(lksystem::core::SIGKILL),
            b'p' => {
                self.stop(lksystem::core::SIGSTOP);
                if let Some(process) = &mut self.process {
                    process.paused = true;
                }
            }
            b'c' => {
                self.stop(lksystem::core::SIGCONT);
                if let Some(process) = &mut self.process {
                    process.paused = false;
                }
            }
            b'o' => {
                self.want = Want::Down;
                if self.process.is_none() {
                    let _ = self.start_run();
                }
            }
            b'a' => self.stop(lksystem::core::SIGALRM),
            b'h' => self.stop(lksystem::core::SIGHUP),
            b'i' => self.stop(lksystem::core::SIGINT),
            b'q' => self.stop(lksystem::core::SIGQUIT),
            b'1' => self.stop(lksystem::core::SIGUSR1),
            b'2' => self.stop(lksystem::core::SIGUSR2),
            _ => {}
        }
        let _ = self.publish();
    }

    fn poll(&mut self) -> io::Result<bool> {
        for byte in read_available(&mut self.control)? {
            self.apply_control(byte);
        }
        if take_terminate() {
            self.apply_control(b'x');
        }

        let finished = match &mut self.process {
            Some(process) => process.child.try_wait()?,
            None => None,
        };
        if let Some(exit_status) = finished {
            let phase = self.process.as_ref().unwrap().phase;
            if phase == Phase::Run {
                self.start_finish(exit_status)?;
            } else {
                self.process = None;
                self.retry_at = Instant::now() + Duration::from_secs(1);
                self.publish()?;
            }
        }

        if self.process.is_none() && self.want == Want::Up && Instant::now() >= self.retry_at {
            self.start_run()?;
        }
        if self.want == Want::Exit && self.process.is_none() {
            if let Some(logger) = &mut self.logger {
                let _ = logger.kill();
            }
            return Ok(true);
        }
        Ok(false)
    }
}

fn main() -> io::Result<()> {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(service) = args.next() else { usage() };
    if args.next().is_some() {
        usage();
    }
    install_signal_handlers();
    let mut supervisor = Supervisor::new(PathBuf::from(service))?;
    loop {
        if supervisor.poll()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
}
