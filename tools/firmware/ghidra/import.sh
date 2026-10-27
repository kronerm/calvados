#!/bin/sh
# Import and analyze firmware images into one Ghidra project (Thumb-2,
# little-endian, loaded at 0; offsets in docs/firmware.md are file offsets).
#
#   tools/firmware/ghidra/import.sh firmware/A3048/parts/FTAB/rkof.bin …
#
# Takes a few minutes per image.
set -eu
mkdir -p "${PROJECT:-firmware/ghidra}"
for image in "$@"; do
    "${GHIDRA:-/opt/ghidra}/support/analyzeHeadless" "${PROJECT:-firmware/ghidra}" fw \
        -import "$image" -loader BinaryLoader -loader-baseAddr 0 \
        -processor ARM:LE:32:Cortex -overwrite
done
