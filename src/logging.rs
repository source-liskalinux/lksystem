pub fn setup_logging(conf: &crate::config::LoggingConfig) -> Result<(), String> {
    let mut logger = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Trace);

    if conf.log_to_stdout {
        logger = logger.chain(std::io::stdout());
    }

    if conf.log_to_disk {
        if let Err(e) = std::fs::create_dir_all(&conf.log_dir) {
            return Err(format!("Could not create log directory {:?}: {}", conf.log_dir, e));
        }
        let log_file_path = conf.log_dir.join("lksystem.log");
        let log_file = fern::log_file(&log_file_path)
            .map_err(|e| format!("Could not open log file {:?}: {}", log_file_path, e))?;
        logger = logger.chain(log_file);
    }

    logger
        .apply()
        .map_err(|e| format!("Error while setting up logger: {}", e))
}
