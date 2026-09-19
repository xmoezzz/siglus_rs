#!/bin/sh
# Switch build entry: builds the probe .nro from the vendored toolchain stack.
# Requires devkitPro (DEVKITPRO=/opt/devkitpro).
set -e
export DEVKITPRO=/opt/devkitpro
export DEVKITARM=/opt/devkitpro/devkitA64
export PATH="/opt/devkitpro/devkitA64/bin:/opt/devkitpro/tools/bin:$PATH"
cd "$(dirname "$0")/probe"
cargo nx build "$@"
ls -la target/aarch64-nintendo-switch-freestanding/*/siglus_switch_probe.nro
