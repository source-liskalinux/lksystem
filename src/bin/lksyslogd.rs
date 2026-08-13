use lksystem::core::{install_signal_handlers, take_reload, take_terminate};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct LogDirectory {
    path: PathBuf,
    current: File,
    size: u64,
    limit: u64,
    timestamp: u8,
}

fn usage() -> ! {
    eprintln!("usage: lksyslogd [-tttvL] [-l len] [-b buflen] dir ...");
    std::process::exit(111);
}

fn settings(path: &Path) -> (u64, usize) {
    let config = fs::read_to_string(path.join("config")).unwrap_or_default();
    let mut size = 1_000_000;
    let mut keep = 10_usize;
    for line in config.lines() {
        if let Some(value) = line.strip_prefix("s") {
            size = value.trim().parse().unwrap_or(size);
        }
        if let Some(value) = line.strip_prefix("n") {
            keep = value.trim().parse().unwrap_or(keep);
        }
    }
    (size.max(1), keep)
}

fn rotate(path: &Path, keep: usize) -> io::Result<File> {
    let current = path.join("current");
    if current.exists() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        fs::rename(
            &current,
            path.join(format!(
                "@{:010}.{:09}.s",
                now.as_secs(),
                now.subsec_nanos()
            )),
        )?;
    }
    let mut archives: Vec<_> = fs::read_dir(path)?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with('@'))
        .collect();
    archives.sort_by_key(|entry| entry.file_name());
    while archives.len() > keep {
        let entry = archives.remove(0);
        let _ = fs::remove_file(entry.path());
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o644)
        .open(current)
}

impl LogDirectory {
    fn open(path: PathBuf, timestamp: u8) -> io::Result<Self> {
        fs::create_dir_all(&path)?;
        let (limit, _) = settings(&path);
        let current_path = path.join("current");
        let current = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o644)
            .open(&current_path)?;
        let size = current.metadata()?.len();
        Ok(Self {
            path,
            current,
            size,
            limit,
            timestamp,
        })
    }
    fn write(&mut self, line: &[u8]) -> io::Result<()> {
        let (limit, keep) = settings(&self.path);
        self.limit = limit;
        if self.size.saturating_add(line.len() as u64 + 1) > self.limit {
            self.current.flush()?;
            self.current = rotate(&self.path, keep)?;
            self.size = 0;
        }
        if self.timestamp != 0 {
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            write!(self.current, "{seconds} ")?;
        }
        self.current.write_all(line)?;
        self.current.write_all(b"\n")?;
        self.current.flush()?;
        self.size += line.len() as u64 + 1;
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut arguments = env::args().skip(1).peekable();
    let mut timestamp = 0_u8;
    while let Some(argument) = arguments.peek() {
        if argument == "-t" {
            timestamp = timestamp.saturating_add(1).min(3);
            arguments.next();
        } else if argument == "-l" || argument == "-b" || argument == "-r" || argument == "-R" {
            arguments.next();
            let _ = arguments.next();
        } else if argument.starts_with('-') {
            usage();
        } else {
            break;
        }
    }
    let directories: Vec<_> = arguments.map(PathBuf::from).collect();
    if directories.is_empty() {
        usage();
    }
    install_signal_handlers();
    let mut outputs: Vec<LogDirectory> = directories
        .into_iter()
        .map(|directory| LogDirectory::open(directory, timestamp))
        .collect::<io::Result<_>>()?;
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    loop {
        let mut line = Vec::new();
        let length = input.read_until(b'\n', &mut line)?;
        if length == 0 || take_terminate() {
            return Ok(());
        }
        if take_reload() {
            continue;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        for output in &mut outputs {
            output.write(&line)?;
        }
    }
}
