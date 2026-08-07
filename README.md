# Liska System Manager

Lksystem is a Linux init and service manager for Liska Linux. It aims to provide a systemd-style experience without depending on systemd itself, but it is not yet a full systemd replacement.

The current implementation covers a practical core subset of init and service-management behavior:

- PID1-style startup flow, including early mounting of basic virtual filesystems such as `/proc`, `/sys`, `/dev`, and `/run`
- unit loading and parsing for services, sockets, timers, devices, paths, and targets
- basic dependency handling such as `After`, `Before`, `Wants`, `Requires`, and `Conflicts`
- service start/stop/reload/restart handling, including simple, oneshot, notify, and D-Bus-style services
- basic control over Unix sockets and D-Bus through `lksystemctl`
- cgroup-based process grouping on Linux, enabled by default for service isolation and resource-aware startup
- propagation of `Environment=` and `EnvironmentFile=` values so desktop-oriented services can receive Hyprland/Wayland-related variables

The project is still maturing. It does not yet implement the full systemd feature set, especially around advanced unit semantics, drop-in overrides, templates, full seat/session management, or logind-style behavior. For Liska Linux, however, the current scope is already sufficient for boot-time service management and desktop-oriented service startup with Hyprland and SDDM in mind.

Control and management are exposed through the `lksystemctl` CLI, which can interact with the daemon over a Unix socket or via D-Bus. This is the intended replacement for `systemctl` in the lksystem environment.

Lksystem is not a wrapper around systemd; it is a separate implementation designed to be compatible with systemd-style concepts and management workflows, but not built on top of systemd.