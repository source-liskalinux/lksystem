# Liska System Manager

Liska System Manager is a Linux init and service supervision toolkit for Liska Linux. The primary PID 1 binary is `lksystem`, the early init
stages are small Rust programs installed under `/etc/lksystem`, and the
runit-compatible supervisor (`lksys`, `lksysdir`, `lksysctl`, `lksyschdir`,
`lksyslogd`) is a native Rust reimplementation of runit's on-disk protocol.
The same 20-byte `supervise/status` layout, the same `control`/`ok` FIFOs, and
the same `down`/`run`/`finish`/`log` service-directory conventions.

The repository currently builds:
- `lksystem`, `lksys`, `lksysdir`, `lksysctl`,
  `lksyschdir`, `lksyslogd`, and `chpst` (located at `src/`).
- `lksystem-stage1`, `lksystem-stage2`, `lksystem-stage3`, and
  `lksystem-ctrlaltdel` (installed as
  `/etc/lksystem/{1,2,3,ctrlaltdel}` and located at `etc/src/`).

`chpst` covers the common runit/daemontools surface used by `run` scripts:
`-u`/`-U` (setuidgid/envuidgid, including the `:uid:gid` numeric-only form),
`-e` (envdir), `-/` (chroot), `-n` (nice), `-P` (new session via `setsid`,
used by the bundled `agetty-tty*` services so busybox getty is a proper
session leader for its console), `-0`/`-1`/`-2` (close std fds), `-l`/`-L`
(flock-based locking held across `exec`), `-b` (argv0), and the resource
limits `-m -d -o -p -f -c -t` (`-s` for `RLIMIT_STACK` is a non-standard
extension).

## Build
```sh
make
make check
```

`make` builds both Cargo crates (the `src/` tools and the `etc/src`
boot-stage programs) in release mode. `make check` additionally runs 
`cargo test` for both crates. Both crates depend on [`colored`](https://crates.io/crates/colored) for terminal output. `cargo build` fetches it from crates.io 
on first build.

## Install
Install into a package staging directory:
```sh
make install DESTDIR="$pkgdir" PREFIX=/usr
```

With `PREFIX=/usr`, the public `lksys*` binaries and `lksystem` are installed
below `/usr/sbin`. The stage programs are installed at the fixed paths used by
`lksystem`: `/etc/lksystem/1`, `/etc/lksystem/2`, `/etc/lksystem/3`, and
`/etc/lksystem/ctrlaltdel`. `example-initramfs.rs` is not installed by `make
install` by default, build and place it manually per
[`docs/initramfs.md`](docs/initramfs.md) only when building an initramfs.

Bundled service directories are installed below `/etc/lksystem/services`:
- `dbus`: starts the system bus in the foreground.
- `networkmanager`: waits for D-Bus before starting NetworkManager.
- `agetty-tty1` through `agetty-tty8`: provide virtual-console logins, `agetty-tty1` is the default login prompt after boot.

Make sure the target system provides the programs those services call, or
remove services that are not needed before booting.

## UI implementation
Liska System Manager use the UI module in `etc/src/ui.rs` and `src/ui.rs`.
Both emit plain text when stderr is not a terminal and ANSI-colored status
messages when it is safe to do so.

## Initramfs
For boot integration, use a small initramfs `/init` to mount the real root and
then `exec switch_root "$NEWROOT" /usr/sbin/lksystem`. See
[`docs/initramfs.md`](docs/initramfs.md) for a full example, packaging notes,
and a verification checklist.
