#!/usr/bin/env bash
# The client uses its own Tier 3 std and DX9 feature; no modern toolchain/default is lowered.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
toolchain=${WIN7_TOOLCHAIN:-nightly-2026-11-11}
target=x86_64-win7-windows-gnu
work=${WIN7_BUILD_ROOT:-"$repo/work/win7-client"}
mkdir -p "$work"
work=$(cd -- "$work" && pwd)
export CARGO_HOME=${CARGO_HOME:-"$work/cargo"}
export RUSTUP_HOME=${RUSTUP_HOME:-"$work/rustup"}
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$work/target"}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
cross=${WIN7_CROSS_ROOT:-"$work/cross"}
export PATH="$cross/usr/bin:$PATH"
cc=${WIN7_CC:-"$cross/usr/bin/x86_64-w64-mingw32-gcc-posix"}
command -v rustup >/dev/null || { echo 'Install rustup first (use the work-local CARGO_HOME and RUSTUP_HOME).' >&2; exit 1; }
[[ -x "$cc" ]] || { echo "Run tools/bootstrap_win7_cross.sh '$cross' first (or set WIN7_CC)." >&2; exit 1; }
rustup toolchain install "$toolchain" --profile minimal --component rust-src
export CARGO_TARGET_X86_64_WIN7_WINDOWS_GNU_LINKER="$cc"
export CC_x86_64_win7_windows_gnu="$cc"
export AR_x86_64_win7_windows_gnu=x86_64-w64-mingw32-ar
export WINDRES="$cross/usr/bin/x86_64-w64-mingw32-windres"
unset CARGO_ENCODED_RUSTFLAGS
# N455 supports SSE3 and SSSE3; never inherit the build host's SSE4/POPCNT/AVX.
export RUSTFLAGS='--cfg windows_slim_errors -C target-cpu=x86-64 -C target-feature=+sse3,+ssse3,+crt-static -C panic=abort -C link-arg=-Wl,--major-subsystem-version,6 -C link-arg=-Wl,--minor-subsystem-version,1 -C link-arg=-static'
cd -- "$repo"
# Older windows-targets build scripts do not recognize the win7 vendor. Supply their
# checksum-verified import libraries without patching the registry or the modern build.
cargo "+$toolchain" metadata --locked --format-version 1 --filter-platform "$target" \
    --no-default-features --features horizon-client/legacy-d3d9,horizon-client/audio > "$work/cargo-metadata.json"
mapfile -t import_dirs < <(python3 - "$work/cargo-metadata.json" <<'PY'
import json, pathlib, sys
for package in json.loads(pathlib.Path(sys.argv[1]).read_text())['packages']:
    if package['name'] == 'windows_x86_64_gnu':
        print(pathlib.Path(package['manifest_path']).parent / 'lib')
PY
)
link_args=()
for directory in "${import_dirs[@]}"; do link_args+=(-L "native=$directory"); done
cargo "+$toolchain" rustc --locked --profile legacy -p horizon-client --bin horizon-client \
    --target "$target" -Z build-std=std,panic_abort \
    --no-default-features --features legacy-d3d9,audio -- "${link_args[@]}"
# Ship the existing local diagnostics client: no separate profiler installation.
cargo "+$toolchain" rustc --locked --profile legacy -p horizon-client-lib --bin horizon-ctl \
    --target "$target" -Z build-std=std,panic_abort \
    --no-default-features --features legacy-d3d9,audio -- "${link_args[@]}"
cargo "+$toolchain" rustc --locked --profile legacy -p horizon-client-d3d9 --example smoke \
    --target "$target" -Z build-std=std,panic_abort -- "${link_args[@]}"
binary="$CARGO_TARGET_DIR/$target/legacy/horizon-client.exe"
smoke="$CARGO_TARGET_DIR/$target/legacy/examples/smoke.exe"
control="$CARGO_TARGET_DIR/$target/legacy/horizon-ctl.exe"
python3 tools/check_win7_imports.py --d3d9 --json "$binary" > "$work/imports-client-win7-x64.json"
python3 tools/check_win7_imports.py --d3d9 --json "$smoke" > "$work/imports-smoke-win7-x64.json"
python3 tools/check_win7_imports.py --json "$control" > "$work/imports-control-win7-x64.json"
out=${1:-"$work/artifacts"}
mkdir -p "$out"
out=$(cd -- "$out" && pwd)
rustc "+$toolchain" -Vv > "$out/build-info.txt"
{ git rev-parse HEAD; "$cc" --version | head -1; printf '%s\n' "$target" "$RUSTFLAGS"; } >> "$out/build-info.txt"
cp "$work/imports-client-win7-x64.json" "$work/imports-smoke-win7-x64.json" "$work/imports-control-win7-x64.json" "$out/"
python3 - "$binary" "$smoke" "$control" "$repo/packaging/win7-client/TESTING.txt" "$out" <<'PY'
import hashlib, pathlib, sys, zipfile
binary, smoke, control, instructions, out = map(pathlib.Path, sys.argv[1:])
archive = out / 'horizon-client-win7-x64.zip'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as package:
    package.write(binary, 'horizon-client.exe')
    package.write(smoke, 'd3d9-smoke.exe')
    package.write(control, 'horizon-ctl.exe')
    package.write(instructions, 'TESTING.txt')
    package.write(out / 'build-info.txt', 'build-info.txt')
(out / 'SHA256SUMS').write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n', encoding='utf-8')
PY
printf '%s\n' "Built $out/horizon-client-win7-x64.zip"
