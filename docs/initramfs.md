# Integrating lksystem with an initramfs

`lksystem` must be executed by the kernel as PID 1. It uses the fixed stage
paths `/etc/lksystem/1`, `/etc/lksystem/2`, and `/etc/lksystem/3`, its stage-2
program also needs `lksysdir` available in `PATH`. The usual design is to use
a small initramfs `/init` to find and mount the real root filesystem, then
`exec switch_root` into `/usr/sbin/lksystem` on that root.

This document assumes an x86-64 Linux host and a root filesystem that already
contains the installed lksystem package. Adapt the root device, filesystem,
and any LUKS/LVM/network setup to the machine.

## Build the Rust `/init` example statically
[`example-initramfs.rs`](../example-initramfs.rs) must be statically linked
when it is copied into an initramfs. A normal Rust build needs the system
dynamic loader before the program can start. That loader usually is not in the
early archive, causing the kernel to report that it cannot execute init and
panic before the program can show an error.

Compile it through Cargo with Rust static C runtime option:
```sh
cargo rustc --manifest-path etc/Cargo.toml --release \
  --bin lksystem-initramfs -- -C target-feature=+crt-static
install -Dm0755 etc/target/release/example-initramfs "$INITRAMFS/init"
```

WARNING: Do not omit the `-C target-feature=+crt-static` argument for a `/init` build!
Without it, the resulting file is dynamically linked. Verify the generated
binary with `file "$INITRAMFS/init"`, it must report `static-pie linked` (or
`statically linked`) and must be executable.

## 1. Build and install lksystem into the target root
From the repository root, build and install into a staged copy of the final
root filesystem:
```sh
make check
make DESTDIR="$ROOTFS" PREFIX=/usr install
```

This installs the supervisor at `/usr/sbin/lksystem`, native tools such as
`lksysdir` and `lksysctl` under `/usr/sbin`, and the Rust stage programs at
`/etc/lksystem/{1,2,3,ctrlaltdel}`. The installation also creates the bundled
service directories below `/etc/lksystem/services`.

Ensure the actual root filesystem provides every executable called by the
stages and enabled services: `mount`, `wall`, a POSIX shell, `dbus-daemon`,
`NetworkManager`, BusyBox (for the bundled `getty-tty*` services, which run
`busybox getty`), and their shared libraries. Configure or remove the
supplied services before booting if those programs are not wanted.

Stage 2 also starts the udev device manager (`udevd` plus `udevadm trigger`
and `udevadm settle`) and then runs `mount -a` against `/etc/fstab`, before
switching the console and handing off to `lksysdir`. Both steps are
best-effort, a missing `udevd`/`udevadm`, no `/etc/fstab`, or a mount
failure is logged as a warning and does not stop boot. The target root must
provide `udevd` and `udevadm` (one of `/usr/lib/udev/udevd`,
`/lib/udev/udevd`, `/usr/sbin/udevd`, or `/sbin/udevd`, e.g. from `eudev` package)
plus a populated `/etc/fstab` for these to do anything. Set
`LKSYSTEM_SKIP_UDEV=1` or `LKSYSTEM_SKIP_FSTAB=1` in the init file
to disable either step before compiling, e.g. for a chroot or container build.

## 2. Create the early `/init`
The following minimal `/init` uses BusyBox to mount a known root partition.
Save it as `$INITRAMFS/init` and mark it executable. `ROOT_DEVICE` must name a
device that exists after the kernel has loaded the required storage driver.
For example:
```sh
#!/bin/busybox sh
set -eu

ROOT_DEVICE=/dev/vda2
ROOT_FILESYSTEM=ext4
NEWROOT=/newroot

mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mkdir -p "$NEWROOT"
mount -t "$ROOT_FILESYSTEM" "$ROOT_DEVICE" "$NEWROOT"

exec switch_root "$NEWROOT" /usr/sbin/lksystem
```

At minimum, the initramfs must contain `/bin/busybox`, an `/init` symlink or
shebang that invokes it, and the BusyBox applets used above (`sh`, `mount`,
`mkdir`, and `switch_root`). For a module-based storage stack, include the
needed kernel modules and load them before mounting `ROOT_DEVICE`. For
encrypted or LVM roots, unlock/activate them in this script first.

`switch_root` is important: it replaces the initramfs root while keeping
`lksystem` as PID 1. Do not start `lksystem` in the background, and do not use
a shell script that exits after invoking it.

The packaged binary lives at `/usr/sbin/lksystem`. If `/init` is instead the
statically-linked `example-initramfs.rs` example built above, it searches the
new root for `/usr/sbin/lksystem`, then a handful of other common init paths,
before calling `switch_root`. If none of them can be found or exec fails, it
falls back to an emergency shell on the console instead of exiting and
triggering a kernel panic, but the image still needs a valid static binary
(or its dynamic loader and libraries) before it can boot normally.

## 3. Pack the initramfs
Create the archive from inside the initramfs directory so archive entries use
absolute-root-relative paths:
```sh
cd "$INITRAMFS"
find . -print0 | cpio --null -o --format=newc | gzip -9 > ../initramfs-linux.img
```

With a Linux kernel image and an appropriate command line, QEMU can be used
for a basic boot test:
```sh
qemu-system-x86_64 \
  -kernel /boot/vmlinuz-linux \
  -initrd ./initramfs-linux.img \
  -append 'console=ttyS0 root=/dev/vda2' \
  -drive file=rootfs.img,format=raw,if=virtio \
  -nographic
```

The example `/init` uses `/dev/vda2`; make that agree with the disk layout
passed to QEMU. On real hardware, add the initramfs to the bootloader's
`initrd` line and pass the correct root-device argument if your early script
uses it.

## Standalone initramfs option
It is possible to place the entire lksystem installation directly in the
initramfs and set `/init` to `exec /usr/sbin/lksystem`. This is useful for a
rescue image, but all lksystem executables and every shared library they need
must then be included in the archive. Inspect each binary with `ldd` after
building and copy both the reported libraries and the dynamic loader. A
missing loader or library causes PID 1 startup to fail before any UI message
can be printed.

For normal systems, the early-wrapper approach above is preferable: keep the
initramfs small, and keep lksystem, service definitions, and their runtime
dependencies on the real root filesystem.

## Verification checklist
- [x] The kernel command line exposes the intended root device and console.
- [x] `/init` is executable and its interpreter exists in the archive.
- [x] Storage drivers, firmware, and modules required for the root device are
  available before the root mount.
- [x] The target root contains `/usr/sbin/lksystem`, all four stage files, and
  `/usr/sbin/lksysdir`.
- [x] The target root contains the selected service programs and shared-library
  dependencies.
- [x] On virtual-console boots, `tty1` becomes the default login console after
  stage 2 hands off to the service supervisor.
- [x] Serial-console output shows the lksystem welcome banner after `switch_root`.
