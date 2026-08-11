# Liska System Manager

Liska System Manager is a Linux init and service supervision toolkit. The
primary PID 1 binary is `lksystem`, the early init stages are small Rust
programs installed under `/etc/lksystem`, while the supervisor and control
tools are native C binaries.

The repository currently builds:
- `lksystem`: the PID 1 supervisor.
- `lksystem-init`: helper init entry point.
- `lksys`, `lksysdir`, `lksysctl`, `lksyschdir`, and `lksyslogd`: service
  management tools.
- `chpst`: process environment and privilege helper.
- `lksystem-stage1`, `lksystem-stage2`, `lksystem-stage3`, and
  `lksystem-ctrlaltdel`: Rust stage programs installed as
  `/etc/lksystem/{1,2,3,ctrlaltdel}`.

## Build

```sh
make
make check
```
`make check` builds the C tools, runs their local checks, smoke-tests the
native C UI, and runs the Rust test suite.

## Install

Install into a package staging directory:
```sh
make install DESTDIR="$pkgdir" PREFIX=/usr
```
With `PREFIX=/usr`, the C and Rust binaries are installed below `/usr/sbin`.
The stage programs are also installed at the fixed paths used by `lksystem`:
`/etc/lksystem/1`, `/etc/lksystem/2`, `/etc/lksystem/3`, and
`/etc/lksystem/ctrlaltdel`.

Bundled service directories are installed below `/etc/lksystem/service`.
Their `run` files are POSIX shell scripts:
- `dbus`: starts the system bus in the foreground.
- `networkmanager`: waits for D-Bus before starting NetworkManager.
- `getty-tty1` through `getty-tty8`: provide virtual-console logins.
  Stage 2 switches the active virtual console to `tty1` before starting the
  supervisor, so `getty-tty1` is the default login prompt after boot.

Make sure the target system provides the programs those services call, or
remove services that are not needed before booting.

## UI implementation

`lksystem` uses the native C UI in `src/ui.c` and `src/ui.h`. The Rust stages
use the matching Rust UI module in `etc/src/ui.rs`. Both implementations emit
plain text when stderr is not a terminal and ANSI-colored status messages when
it is safe to do so.

## Initramfs

For boot integration, use a small initramfs `/init` to mount the real root and
then `exec switch_root "$NEWROOT" /usr/sbin/lksystem`. See
[`docs/initramfs.md`](docs/initramfs.md) for a full example, packaging notes,
and a verification checklist.
