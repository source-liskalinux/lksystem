pkgname=lksystem
pkgver=1.0.0
pkgrel=1
pkgdesc='Linux-focused service supervision suite with Rust init stages and native C tools'
arch=('x86_64')
url='https://github.com/source-liskalinux/lksystem'
license=('BSD-3-Clause')
depends=('dbus' 'glibc' 'networkmanager' 'util-linux')
makedepends=('rust' 'gcc' 'make')
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

# Keep makepkg's internal $srcdir away from this repository's real ./src tree.
if [[ -z ${BUILDDIR:-} || ${BUILDDIR:-} -ef "$startdir" ]]; then
  BUILDDIR="$startdir/.makepkg"
fi

# This PKGBUILD packages the checked-out local source tree. Run makepkg or
# lkmake from the repository root.
build() {
    make PREFIX=/usr
}

check() {
    make check
}

package() {
    make DESTDIR="$pkgdir" PREFIX=/usr install
    install -Dm644 COPYING "$pkgdir/usr/share/licenses/$pkgname/COPYING"
}
