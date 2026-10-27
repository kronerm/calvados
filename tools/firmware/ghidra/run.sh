#!/bin/sh
# Query a Ghidra project with Dump.java, read-only (~8 s per call).
#
#   tools/firmware/ghidra/import.sh firmware/A3048/parts/FTAB/rkof.bin   once
#   tools/firmware/ghidra/run.sh dec 0x6cf48           decompile
#   tools/firmware/ghidra/run.sh str 'UARPL3'          strings + references
#   (modes: see Dump.java)
#
# GHIDRA defaults to /opt/ghidra, PROJECT to firmware/ghidra, PROGRAM rkof.bin.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
"${GHIDRA:-/opt/ghidra}/support/analyzeHeadless" "${PROJECT:-firmware/ghidra}" fw \
    -process "${PROGRAM:-rkof.bin}" -noanalysis -readOnly \
    -scriptPath "$here" -postScript Dump.java "$@" 2>&1 |
    sed -n 's/^.*@@//p' | sed 's/ (GhidraScript) *$//'
