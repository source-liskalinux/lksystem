# PKGBUILD For lksystem

# Contributor: Janorovic Volkov <janorovicvolkov@gmail.com>
# Maintainer: Janorovic Volkov <janorovicvolkov@gmail.com>

pkgname=lksystem
pkgver=1.0.0
pkgrel=1
pkgdesc='Linux-focused runit-compatible service supervision system manager for Liska Linux'
arch=('x86_64')
url='https://github.com/source-liskalinux/lksystem'
license=('BSD-3-Clause')
depends=('dbus' 'glibc' 'networkmanager' 'util-linux' 'busybox' 'eudev')
makedepends=('rustup' 'make')
conflicts=('runit')
backup=(
    'etc/lksystem/service/dbus/run'
    'etc/lksystem/service/networkmanager/run'
    'etc/lksystem/service/getty-tty1/run'
    'etc/lksystem/service/getty-tty2/run'
    'etc/lksystem/service/getty-tty3/run'
    'etc/lksystem/service/getty-tty4/run'
    'etc/lksystem/service/getty-tty5/run'
    'etc/lksystem/service/getty-tty6/run'
    'etc/lksystem/service/getty-tty7/run'
    'etc/lksystem/service/getty-tty8/run'
)
source=()
sha256sums=()

# Keep makepkg's internal $srcdir away from this repository's real ./src tree
if [[ -z ${BUILDDIR:-} || ${BUILDDIR:-} -ef "$startdir" ]]; then
  BUILDDIR="$startdir/.makepkg"
fi

# This PKGBUILD packages the checked-out local source tree. Run makepkg or
# lkmake from the repository root
build() {
    rustup target add x86_64-unknown-linux-musl
    make PREFIX=/usr
}

check() {
    make check
}

package() {
    make DESTDIR="$pkgdir" PREFIX=/usr install
    # /sbin and /usr/sbin are both symlinks to /usr/bin (see the
    # `filesystem` package), so placing these directly in /usr/bin makes
    # them reachable as /sbin/reboot, /usr/sbin/reboot, and
    # /usr/bin/reboot alike. Deliberately NOT creating /sbin or /usr/sbin
    # as real directories here, that would conflict with filesystem's
    # symlink declarations for those paths, the same class of bug fixed
    # earlier in eudev's PKGBUILD (dir vs symlink ownership clash).
    install -d -m755 "${pkgdir}/usr/bin"
    for cmd in reboot shutdown poweroff halt; do
        ln -sf /usr/sbin/lksysctl "${pkgdir}/usr/bin/${cmd}"
    done
}
