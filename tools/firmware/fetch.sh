#!/bin/sh
# Fetch the current firmware of one AirPods model from Apple's asset catalog,
# unpack it and extract its images. Apple serves only the current build.
#
#   tools/firmware/fetch.sh MODEL [OUT]    e.g. A3048 (AirPods Pro 2 USB-C)
#
# Prints the build and BLAKE3 hashes to compare with docs/firmware.md.
# Needs curl, unzip, python3, b3sum. OUT defaults to firmware/ (gitignored).
set -eu
model=$1
out=${2:-firmware}/$model
here=$(dirname "$0")
asset=com_apple_MobileAsset_UARP_$model
mkdir -p "$out"
curl -fsS -o "$out/catalog.xml" "https://mesu.apple.com/assets/$asset/$asset.xml"
url=$(python3 -I - "$out/catalog.xml" <<'PY'
import plistlib, sys
a = plistlib.load(open(sys.argv[1], "rb"))["Assets"][0]
print(a["Build"], file=sys.stderr)
print(a["__BaseURL"] + a["__RelativePath"])
PY
)
curl -fsS -o "$out/asset.zip" "$url"
rm -rf "$out/asset"
unzip -q "$out/asset.zip" -d "$out/asset"
uarp=$(find "$out/asset" -name firmware.uarp)
python3 -I "$here/extract.py" "$uarp" "$out/parts"
b3sum "$out/asset.zip" "$uarp" "$out"/parts/FTAB/*.bin
