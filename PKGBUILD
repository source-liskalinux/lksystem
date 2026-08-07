# PKGBUILD For lksystem

# Contributor: Janorovic Volkov <janorovicvolkov@gmail.com>
# Maintainer: Janorovic Volkov <janorovicvolkov@gmail.com>

pkgname=lksystem
pkgver=1.0.0
pkgrel=1
pkgdesc="Liska System Manager. A system manager for Liska Linux"
arch=('x86_64')
url="https://github.com/source-liskalinux/lksystem"
license=('GPL-3.0-or-later')
depends=('dbus' 'glibc')
optdepends=('sddm' 'hyprland')
makedepends=('rust')
conflicts=('systemd')

build() {
    echo "--> [BUILD] Compiling lksystem...."
    cargo build --release
}

package() {
    install -Dm0755 "./target/release/lksystem" "$pkgdir/usr/bin/lksystem"
    install -Dm0755 "./target/release/lksystemctl" "$pkgdir/usr/bin/lksystemctl"
    install -dm0755 "$pkgdir/etc/lksystem"
    cp -a "./boot_units/." "$pkgdir/etc/lksystem/system"
    install -m0644 "./config/config.lua" "$pkgdir/etc/lksystem/config.lua"
}
