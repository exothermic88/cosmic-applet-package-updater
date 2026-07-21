# Maintainer: nic <nicolasciampa2001@gmail.com>
# Personal-repo PKGBUILD: builds from this local checkout (run `makepkg -f` in the
# repo root). Note that uncommitted changes in the working tree get packaged.
pkgname=ncos-package-update-applet
pkgver=2.0.0
pkgrel=2
pkgdesc="COSMIC panel applet showing pending pacman, AUR and Flatpak updates"
arch=('x86_64' 'aarch64')
url="https://github.com/Ebbo/cosmic-applet-package-updater"
license=('GPL-3.0-only')
depends=('pacman-contrib' 'libxkbcommon' 'wayland')
optdepends=('paru: AUR update checking and updating'
            'yay: AUR update checking and updating (alternative)'
            'flatpak: Flatpak update checking and updating'
            'cosmic-term: default terminal for running updates')
makedepends=('cargo' 'just' 'git')
provides=('cosmic-ext-applet-package-updater')
conflicts=('cosmic-ext-applet-package-updater'
           'cosmic-applet-package-updater-git'
           'cosmic-applet-package-updater')
options=('!lto') # cargo already does fat LTO; avoid makepkg injecting -flto
source=()

build() {
    cd "$startdir"
    just build-release
}

package() {
    cd "$startdir"
    just rootdir="$pkgdir" install
}
