#!/bin/sh
# Set the release version everywhere it lives, then commit and push to main:
# the release workflow builds and publishes v<VERSION> when that tag doesn't
# exist yet.
#
#   tools/release.sh 0.2.0
set -eu
version=$1
case $version in
  *[!0-9.]* | *..* | .* | *.) echo "not a version: $version" >&2; exit 1 ;;
esac
IFS=. read -r major minor patch <<END
$version
END
# cargo-apk2's formula; it must grow with every release for Android to update.
code=$(( (1 << 24) + (major << 16) + (minor << 8) + patch ))
cd "$(dirname "$0")/.."
sed -i "0,/^version = /s/^version = .*/version = \"$version\"/" \
  crates/android-app/Cargo.toml crates/linux/Cargo.toml
sed -i "s/^pkgver=.*/pkgver=$version/; s/^pkgrel=.*/pkgrel=1/" PKGBUILD
sed -i "s/android:versionCode=\"[0-9]*\"/android:versionCode=\"$code\"/; s/android:versionName=\"[^\"]*\"/android:versionName=\"$version\"/" \
  crates/android-app/AndroidManifest.xml
cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null
echo "version $version (versionCode $code); now commit and push to main"
