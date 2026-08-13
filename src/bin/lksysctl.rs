use lksystem::core::{
    open_fifo_writer, read_status, service_path, Status, STATE_DOWN, STATE_RUN, WANT_UP,
};
use lksystem::ui;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn usage() -> ! {
    eprintln!("usage: lksysctl [-v] [-w sec] command service ...");
    std::process::exit(100);
}

fn action(command: &str) -> Option<(&'static [u8], bool)> {
    Some(match command {
        "up" | "start" => (b"u", true),
        "down" | "stop" => (b"d", true),
        "exit" | "shutdown" => (b"x", true),
        "once" => (b"o", true),
        "pause" => (b"p", true),
        "cont" | "continue" => (b"c", true),
        "term" => (b"t", false),
        "kill" => (b"k", false),
        "hup" | "reload" => (b"h", false),
        "alarm" => (b"a", false),
        "int" => (b"i", false),
        "quit" => (b"q", false),
        "usr1" => (b"1", false),
        "usr2" => (b"2", false),
        "restart" => (b"tcu", true),
        "force-stop" => (b"dk", true),
        "force-restart" => (b"tkcu", true),
        "force-shutdown" => (b"xk", true),
        _ => return None,
    })
}

fn tai_seconds(status: Status) -> u64 {
    let raw = u64::from_be_bytes(status.started[..8].try_into().unwrap());
    raw.saturating_sub(4_611_686_018_427_387_914)
}

fn status_line(service: &str, status: Status) -> String {
    let state = match status.state {
        STATE_RUN => "run",
        2 => "finish",
        _ => "down",
    };
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        .saturating_sub(tai_seconds(status));
    let mut line = format!("{state}: {service}: ");
    if status.state != STATE_DOWN {
        line.push_str(&format!("(pid {}) ", status.pid));
    }
    line.push_str(&format!("{elapsed}s"));
    if status.pid != 0 && status.want != WANT_UP {
        line.push_str(", want down");
    }
    if status.paused {
        line.push_str(", paused");
    }
    if status.got_term {
        line.push_str(", got TERM");
    }
    line
}

fn desired(status: Status, actions: &[u8]) -> bool {
    actions.iter().all(|action| match action {
        b'u' => status.pid != 0 && status.state == STATE_RUN && status.want == WANT_UP,
        b'd' | b'x' => status.pid == 0 && status.state == STATE_DOWN,
        b'p' => status.paused,
        b'c' => !status.paused,
        _ => true,
    })
}

fn control(service: &Path, actions: &[u8]) -> io::Result<()> {
    let mut fifo = open_fifo_writer(&service.join("supervise/control"))?;
    fifo.write_all(actions)
}

fn main() -> io::Result<()> {
    let mut arguments = env::args().skip(1).peekable();
    let mut wait = env::var("LKSYSCTL_WAIT")
        .ok()
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .unwrap_or(7);
    let mut verbose = false;
    while let Some(argument) = arguments.peek() {
        match argument.as_str() {
            "-v" => {
                verbose = true;
                arguments.next();
            }
            "-w" => {
                arguments.next();
                wait = arguments
                    .next()
                    .and_then(|seconds| seconds.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            "-V" => {
                println!("lksysctl 1.0.0");
                return Ok(());
            }
            _ if argument.starts_with('-') => usage(),
            _ => break,
        }
    }
    let Some(command) = arguments.next() else {
        usage()
    };
    let services: Vec<_> = arguments.collect();
    if services.is_empty() {
        usage();
    }
    if command == "status" {
        let mut failed = 0;
        for service_name in services {
            let service = service_path(&service_name);
            match read_status(&service, false) {
                Ok(status) => {
                    ui::success(format!("{}", status_line(&service_name, status)));
                    if let Ok(log) = read_status(&service, true) {
                        ui::log(format!("{}", status_line(&format!("{service_name}/log"), log)));
                    }
                }
                Err(error) => {
                    ui::error(format!("> fail: {service_name}: {error}"));
                    failed += 1;
                }
            }
        }
        std::process::exit(failed.min(99));
    }
    let Some((actions, wait_for_state)) = action(&command) else {
        usage()
    };
    let mut failed = 0;
    for service_name in services {
        let service = service_path(&service_name);
        if let Err(error) = control(&service, actions) {
            ui::error(format!("> fail: {service_name}: unable to write supervise or control: {error}"));
            failed += 1;
            continue;
        }
        if wait_for_state || verbose {
            let deadline = Instant::now() + Duration::from_secs(wait);
            loop {
                match read_status(&service, false) {
                    Ok(status) if desired(status, actions) => {
                        if verbose {
                            ui::success(format!("> ok: {}", status_line(&service_name, status)));
                        }
                        break;
                    }
                    Ok(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(200)),
                    Ok(status) => {
                        ui::warning(format!("> timeout: {}", status_line(&service_name, status)));
                        failed += 1;
                        break;
                    }
                    Err(error) => {
                        ui::error(format!("> fail: {service_name}: {error}"));
                        failed += 1;
                        break;
                    }
                }
            }
        }
    }
    let _ = fs::metadata(".");
    std::process::exit(failed.min(99));
}
