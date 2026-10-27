#!/bin/sh
# The build's version from git tags, as `<version> <versionCode>`:
#
#   tag v0.1.1             -> 0.1.1                     (a release)
#   tag v0.1.1-alpha.2     -> 0.1.1alpha2               (a pre-release)
#   3 commits after either -> 0.1.1.r3.g1a2b3c4, 0.1.1alpha2.r3.g1a2b3c4
#
# so pacman's vercmp orders 0.1.0 < 0.1.0.r43.g… < 0.1.1alpha1 < 0.1.1alpha1.r3.g…
# < 0.1.1, and Android's versionCode (an integer) the same way:
# slot = major·10⁴ + minor·10² + patch; a release and the builds after it
# slot·10⁴ + 5000 + n, its alpha k slot·10⁴ + k·100 + n (n capped).
set -eu
cd "$(dirname "$0")/.."
described=$(git describe --tags --long --match 'v[0-9]*' 2>/dev/null) ||
  described="v0.0.0-$(git rev-list --count HEAD)-g$(git rev-parse --short HEAD)"
tag=${described%-*-g*}
rest=${described#"$tag"-}
commits=${rest%%-*}
hash=${rest#*-}
version=${tag#v}
case $version in
  *-alpha.*) base=${version%-alpha.*} alpha=${version#*-alpha.} ;;
  *) base=$version alpha= ;;
esac
IFS=. read -r major minor patch <<EOF
$base
EOF
slot=$((major * 10000 + minor * 100 + patch))
if [ -n "$alpha" ]; then
  name="${base}alpha$alpha"
  code=$((slot * 10000 + alpha * 100 + (commits < 99 ? commits : 99)))
else
  name=$base
  code=$((slot * 10000 + 5000 + (commits < 4999 ? commits : 4999)))
fi
[ "$commits" -eq 0 ] || name="$name.r$commits.$hash"
echo "$name $code"
