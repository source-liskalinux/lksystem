use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("Usage: lksyschdir dir");
    std::process::exit(1);
}

fn main() -> io::Result<()> {
    let mut arguments = env::args_os().skip(1);
    let Some(new) = arguments.next() else { usage() };
    if arguments.next().is_some() {
        usage();
    }
    let new = PathBuf::from(new);
    if new.starts_with(".") || !new.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "new service directory is invalid",
        ));
    }
    let base = env::var_os("lksysDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/lksystem/lksysdir"));
    let target = base.join(&new);
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "service directory does not exist",
        ));
    }
    let current = base.join("current");
    if fs::read_link(&current).ok().as_deref() == Some(new.as_path()) {
        println!("lksyschdir: {}: current.", new.display());
        return Ok(());
    }
    let temporary = base.join("current.new");
    let previous = base.join("previous");
    let _ = fs::remove_file(&temporary);
    std::os::unix::fs::symlink(&new, &temporary)?;
    let _ = fs::remove_file(&previous);
    fs::rename(&current, &previous)?;
    if let Err(error) = fs::rename(&temporary, &current) {
        let _ = fs::rename(&previous, &current);
        return Err(error);
    }
    println!("lksyschdir: {}: now current.", new.display());
    Ok(())
}
