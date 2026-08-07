# lksystem vs systemd feature checklist

This document gives a more practical view of what `lksystem` currently supports and where it still differs from full `systemd` behavior.

`lksystem` is a Linux-native service manager for Liska Linux. Its long-term goal is to become a broadly systemd-compatible manager without depending on systemd itself, while using `lksystemctl` as the replacement for `systemctl`.

## Core features already present
- [x] PID1-style bootstrap and early mount of basic virtual filesystems
- [x] Unit parsing and loading for services, sockets, timers, devices, paths, and targets
- [x] Basic dependency handling for `After`, `Before`, `Wants`, `Requires`, and `Conflicts`
- [x] Service lifecycle handling for start, stop, reload, restart, and oneshot units
- [x] D-Bus control interface and service waiting for D-Bus names
- [x] Basic cgroup integration on Linux, enabled by default
- [x] Environment propagation from `Environment=` and `EnvironmentFile=` values
- [x] Desktop-oriented environment support for Hyprland/Wayland-style services

## Partially implemented or still limited
- [ ] Full systemd unit grammar and semantics
- [ ] Drop-in override directories and merge semantics
- [ ] Template unit expansion
- [ ] Full socket activation semantics
- [ ] Full timer/calendar behavior
- [ ] Advanced cgroup/resource management and slice-style hierarchy features
- [ ] Logind-style seat/session management
- [ ] Full user-session service management and login integration
- [ ] Full journal/logging and introspection features

## Notes
This checklist is intentionally high-level. `systemd` is a very large project, and `lksystem` currently covers a useful core subset of init and service management capabilities. The project is already practical for boot-time service orchestration and desktop-oriented service startup, but it is not yet a complete replacement for every `systemd` feature or subsystem.

For near-term improvements, the highest-value areas are:
1. richer unit dependency relations and activation types
2. more complete socket/timer/device/path activation semantics
3. drop-in override and template unit support
4. better logging, introspection, and cgroup/resource management
