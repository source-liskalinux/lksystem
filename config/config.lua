return {
    logging_dir = "/var/lib/lksystem/logs",
    log_to_stdout = true,
    log_to_disk = false,
    notifications_dir = "/var/lib/lksystem/notifications",
    unit_dirs = {"/etc/lksystem/units"},
    target_unit = "default.target",
    selfpath = "/usr/bin/lksystem",
    sqlite_db = "/var/lib/lksystem/lksystem.db",
}
