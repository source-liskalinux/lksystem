# Integrating lksystem with an initramfs

`lksystem` must be executed by the kernel as PID 1. It uses the fixed stage
paths `/etc/lksystem/1`, `/etc/lksystem/2`, and `/etc/lksystem/3`, its stage-2
program also needs `lksysdir` available in `PATH`. The usual design is to use
a small initramfs `/init` to find and mount the real root filesystem, then
`exec switch_root` into `/usr/sbin/lksystem` on that root.

This document assumes an x86-64 Linux host and a root filesystem that already
contains the installed lksystem package. Adapt the root device, filesystem,
and any LUKS/LVM/network setup to the machine.

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
service directories below `/etc/lksystem/service`.

Ensure the actual root filesystem provides every executable called by the
stages and enabled services: `mount`, `wall`, a POSIX shell, `dbus-daemon`,
`NetworkManager`, `agetty`, and their shared libraries. Configure or remove
the supplied services before booting if those programs are not wanted.

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

## 3. Pack the initramfs

Create the archive from inside the initramfs directory so archive entries use
absolute-root-relative paths:
```sh
cd "$INITRAMFS"
find . -print0 | cpio --null -o --format=newc | gzip -9 > ../lksystem-initramfs.img
```
With a Linux kernel image and an appropriate command line, QEMU can be used
for a basic boot test:
```sh
qemu-system-x86_64 \
  -kernel /boot/vmlinuz-linux \
  -initrd ./lksystem-initramfs.img \
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
- The kernel command line exposes the intended root device and console.
- `/init` is executable and its interpreter exists in the archive.
- Storage drivers, firmware, and modules required for the root device are
  available before the root mount.
- The target root contains `/usr/sbin/lksystem`, all four stage files, and
  `/usr/sbin/lksysdir`.
- The target root contains the selected service programs and shared-library
  dependencies.
- On virtual-console boots, `tty1` becomes the default login console after
  stage 2 hands off to the service supervisor.
- Serial-console output shows the lksystem welcome banner after `switch_root`.
