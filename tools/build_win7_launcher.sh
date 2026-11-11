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
for executable in horizon-launcher-win7 mock-horizon-client; do
    cargo "+$toolchain" rustc --locked --manifest-path launcher/Cargo.toml --release \
        --target "$target" -Z build-std=std,panic_abort \
        --no-default-features --features win7 --bin "$executable" -- -L "native=$import_lib"
done
binary="$CARGO_TARGET_DIR/$target/release/horizon-launcher-win7.exe"
mock="$CARGO_TARGET_DIR/$target/release/mock-horizon-client.exe"
out=${1:-"$work/artifacts"}
mkdir -p "$out"
out=$(cd -- "$out" && pwd)
python3 tools/check_win7_imports.py --json "$binary" > "$out/imports-win7-x64.json"
python3 tools/check_win7_imports.py --json "$mock" > "$out/imports-mock-win7-x64.json"
cp -- "$binary" "$out/horizon-launcher-win7.exe"
cp -- "$mock" "$out/mock-horizon-client.exe"
cp -- packaging/win7/TESTING.md "$out/TESTING.md"
for report in win7-launcher-status.md win7-prepare-audit.md win7-action-parity.md; do
    cp -- "docs/$report" "$out/$report"
done
rustc "+$toolchain" -Vv > "$out/build.txt"
"$cc" -v >> "$out/build.txt" 2>&1
git rev-parse HEAD >> "$out/build.txt"
git diff --stat >> "$out/build.txt"
printf '%s\n' "Target: $target" "RUSTFLAGS=$RUSTFLAGS" >> "$out/build.txt"
sha256sum launcher/Cargo.lock >> "$out/build.txt"
python3 - "$out" <<'PY'
import hashlib
import pathlib
import sys
import zipfile

out = pathlib.Path(sys.argv[1])
names = ('horizon-launcher-win7.exe', 'TESTING.md', 'build.txt', 'imports-win7-x64.json',
         'win7-launcher-status.md', 'win7-prepare-audit.md', 'win7-action-parity.md')
with zipfile.ZipFile(out / 'horizon-launcher-win7-x64.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for name in names:
        archive.write(out / name, name)
with zipfile.ZipFile(out / 'horizon-win7-process-test-x64.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for name in ('mock-horizon-client.exe', 'TESTING.md', 'imports-mock-win7-x64.json'):
        archive.write(out / name, name)
files = (*names, 'horizon-launcher-win7-x64.zip', 'mock-horizon-client.exe',
         'imports-mock-win7-x64.json', 'horizon-win7-process-test-x64.zip')
(out / 'SHA256SUMS.local').write_text(''.join(
    f'{hashlib.sha256((out / name).read_bytes()).hexdigest()}  {name}\n' for name in files
), encoding='utf-8')
PY
echo "Built $out/horizon-launcher-win7-x64.zip. Static checks do not establish Win7 runtime support."
echo 'SHA256SUMS.local is an unsigned local-build checksum list, not a signed release.'
