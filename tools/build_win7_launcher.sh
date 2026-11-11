#!/usr/bin/env bash
# A separate Tier 3 target and std; never lower the modern launcher's toolchain.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
toolchain=nightly-2026-11-01
target=x86_64-win7-windows-gnu
work=${WIN7_BUILD_ROOT:-"$repo/work/win7"}
mkdir -p "$work"
work=$(cd -- "$work" && pwd)
export CARGO_HOME=${CARGO_HOME:-"$work/cargo"}
export RUSTUP_HOME=${RUSTUP_HOME:-"$work/rustup"}
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TARGET_DIR="$work/target"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
cross=${WIN7_CROSS_ROOT:-"$work/cross"}
export PATH="$cross/usr/bin:$PATH"
cc=${WIN7_CC:-"$cross/usr/bin/x86_64-w64-mingw32-gcc-posix"}
command -v rustup >/dev/null || { echo "Install rustup with CARGO_HOME=$CARGO_HOME RUSTUP_HOME=$RUSTUP_HOME first." >&2; exit 1; }
[[ -x "$cc" ]] || { echo "Run tools/bootstrap_win7_cross.sh '$cross' first (or set WIN7_CC)." >&2; exit 1; }
rustup toolchain install "$toolchain" --profile minimal --component rust-src
export CARGO_TARGET_X86_64_WIN7_WINDOWS_GNU_LINKER="$cc"
export CC_x86_64_win7_windows_gnu="$cc"
export AR_x86_64_win7_windows_gnu=x86_64-w64-mingw32-ar
# Pineview has SSE2, but lacks SSE4/POPCNT/AVX. Do not inherit target-cpu=native.
unset CARGO_ENCODED_RUSTFLAGS
export RUSTFLAGS='-C target-cpu=x86-64 -C target-feature=+crt-static -C panic=abort -C link-arg=-Wl,--major-subsystem-version,6 -C link-arg=-Wl,--minor-subsystem-version,1'
cd -- "$repo"
cargo "+$toolchain" tree --locked --manifest-path launcher/Cargo.toml --target "$target" \
    --no-default-features --features win7 --edges normal > "$work/dependency-tree.txt"
if rg -qi '(^|[^[:alnum:]_])(gpui|gpui-fast|wgpu|accesskit_windows)([^[:alnum:]_]|$)' "$work/dependency-tree.txt"; then
    echo 'A GPU/modern UI dependency reached the legacy build; see dependency-tree.txt.' >&2
    exit 1
fi
# windows-targets 0.48.5 predates the win7 vendor targets: its import-library build script
# recognizes only pc/uwp and omits this search path. Use Cargo's checksum-verified package,
# without patching the registry or rebuilding modern targets with different flags.
metadata="$work/cargo-metadata.json"
cargo "+$toolchain" metadata --locked --manifest-path launcher/Cargo.toml \
    --format-version 1 --filter-platform "$target" --no-default-features --features win7 > "$metadata"
import_lib=$(python3 - "$metadata" <<'PY'
import json
import pathlib
import sys

packages = json.loads(pathlib.Path(sys.argv[1]).read_text())['packages']
package = next(p for p in packages
               if p['name'] == 'windows_x86_64_gnu' and p['version'] == '0.48.5')
directory = pathlib.Path(package['manifest_path']).parent / 'lib'
if not (directory / 'libwindows.0.48.5.a').is_file():
    raise SystemExit('Cargo did not provide windows_x86_64_gnu 0.48.5 import libraries')
print(directory)
PY
)
# Cross GCC does not honor LIBRARY_PATH. Give only the final binaries this native path;
# dependencies and their std cache retain the same compiler flags.
cargo "+$toolchain" rustc --locked --manifest-path launcher/Cargo.toml --release \
    --target "$target" -Z build-std=std,panic_abort \
    --no-default-features --features win7 --bin horizon-launcher-win7 -- -L "native=$import_lib"
binary="$CARGO_TARGET_DIR/$target/release/horizon-launcher-win7.exe"
# Keep developer evidence out of the player archive.
python3 tools/check_win7_imports.py --json "$binary" > "$work/imports-win7-x64.json"
out=${1:-"$work/artifacts"}
mkdir -p "$out"
out=$(cd -- "$out" && pwd)
python3 - "$binary" "$repo/packaging/win7/README.md" "$out" <<'PACK'
import hashlib
import pathlib
import sys
import zipfile

binary, readme, out = map(pathlib.Path, sys.argv[1:])
archive = out / 'horizon-launcher-win7-x64.zip'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as package:
    package.write(binary, 'horizon-launcher-win7.exe')
    package.write(readme, 'README.md')
(out / 'SHA256SUMS').write_text(
    f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n', encoding='utf-8')
PACK
printf '%s\n' "Built $out/horizon-launcher-win7-x64.zip"
