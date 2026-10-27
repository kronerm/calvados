# Builds the Linux daemon from this checkout: `makepkg -si`. A same-version
# rebuild reinstalls.
pkgname=calvados
# From git tags (tools/version.sh), like the Android app's version, so both
# order the same: 0.1.0 < 0.1.0.r43.g… < 0.1.1alpha1 < 0.1.1.
pkgver=$(cd "${startdir:-.}" && tools/version.sh | cut -d' ' -f1)
pkgrel=1
pkgdesc='AirPods daemon: battery, ear detection, audio handoff between hosts'
arch=(x86_64 aarch64)
license=(MIT)
# libcap: setcap in the install script.
depends=(bluez dbus gcc-libs glibc libcap libpipewire)
makedepends=(cargo clang)
install=calvados.install
# !lto: makepkg's LTO turns libspa's C shim into bitcode rust-lld can't link.
# !debug: the release build has no debug info, the split package would be empty.
options=(!lto !debug)


prepare() {
  cd "$startdir"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$startdir"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR="$srcdir/target"
  cargo build --frozen --release -p linux
}

check() {
  cd "$startdir"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR="$srcdir/target"
  # --release: reuses the build's dependencies instead of compiling them again.
  cargo test --frozen --release -p protocol -p linux
}

package() {
  install -Dm755 "$srcdir/target/release/calvados" "$pkgdir/usr/bin/calvados"
  sed 's|%h/.local/bin|/usr/bin|' "$startdir/crates/linux/dist/calvados.service" |
    install -Dm644 /dev/stdin "$pkgdir/usr/lib/systemd/user/calvados.service"
  install -d "$pkgdir/usr/share/calvados"
  cp -r --no-preserve=ownership "$startdir/crates/linux/dist/noctalia" "$pkgdir/usr/share/calvados/"
}
