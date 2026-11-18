#!/usr/bin/env bash
# The client uses its own Tier 3 std and DX9 feature; no modern toolchain/default is lowered.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
toolchain=${WIN7_TOOLCHAIN:-nightly-2026-11-11}
architecture=${WIN7_ARCH:-x64}
case "$architecture" in
    x64)
        target=x86_64-win7-windows-gnu
        prefix=x86_64-w64-mingw32
        import_package=windows_x86_64_gnu
        cpu_flags='-C target-cpu=x86-64 -C target-feature=+sse3,+ssse3,+crt-static'
        default_work="$repo/work/win7-client"
        ;;
    x86)
        target=i686-win7-windows-gnu
        prefix=i686-w64-mingw32
        import_package=windows_i686_gnu
        cpu_flags='-C target-cpu=pentium4 -C target-feature=+sse3,+crt-static -C link-arg=-Wl,--large-address-aware'
        default_work="$repo/work/win7-client-x86"
        ;;
    *) echo 'WIN7_ARCH must be x64 or x86.' >&2; exit 2 ;;
esac
work=${WIN7_BUILD_ROOT:-"$default_work"}
mkdir -p "$work"
work=$(cd -- "$work" && pwd)
export CARGO_HOME=${CARGO_HOME:-"$work/cargo"}
export RUSTUP_HOME=${RUSTUP_HOME:-"$work/rustup"}
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$work/target"}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
cross=${WIN7_CROSS_ROOT:-"$work/cross"}
export PATH="$cross/usr/bin:$PATH"
cc=${WIN7_CC:-"$cross/usr/bin/$prefix-gcc-posix"}
command -v rustup >/dev/null || { echo 'Install rustup first (use the work-local CARGO_HOME and RUSTUP_HOME).' >&2; exit 1; }
[[ -x "$cc" ]] || { echo "Run tools/bootstrap_win7_cross.sh '$cross' '$architecture' first (or set WIN7_CC)." >&2; exit 1; }
rustup toolchain install "$toolchain" --profile minimal --component rust-src
target_key=${target//-/_}
export "CARGO_TARGET_${target_key^^}_LINKER=$cc"
export "CC_$target_key=$cc"
export "AR_$target_key=$prefix-ar"
export WINDRES="$cross/usr/bin/$prefix-windres"
unset CARGO_ENCODED_RUSTFLAGS
# x64 retains N455's SSE3/SSSE3; x86 permits SSE3 as requested by the owner.
# Neither inherits the build host's SSE4/POPCNT/AVX.
export RUSTFLAGS="--cfg windows_slim_errors $cpu_flags -C panic=abort -C link-arg=-Wl,--major-subsystem-version,6 -C link-arg=-Wl,--minor-subsystem-version,1 -C link-arg=-static"
cd -- "$repo"
# Older windows-targets build scripts do not recognize the win7 vendor. Supply their
# checksum-verified import libraries without patching the registry or the modern build.
cargo "+$toolchain" metadata --locked --format-version 1 --filter-platform "$target" \
    --no-default-features --features horizon-game/legacy-d3d9,horizon-game/audio > "$work/cargo-metadata.json"
mapfile -t import_dirs < <(python3 - "$work/cargo-metadata.json" "$import_package" <<'PY'
import json, pathlib, sys
for package in json.loads(pathlib.Path(sys.argv[1]).read_text())['packages']:
    if package['name'] == sys.argv[2]:
        print(pathlib.Path(package['manifest_path']).parent / 'lib')
PY
)
link_args=()
for directory in "${import_dirs[@]}"; do link_args+=(-L "native=$directory"); done
cargo "+$toolchain" rustc --locked --profile legacy -p horizon-game --bin horizon-client \
    --target "$target" -Z build-std=std,panic_abort \
    --no-default-features --features legacy-d3d9,audio -- "${link_args[@]}"
binary="$CARGO_TARGET_DIR/$target/legacy/horizon-client.exe"
if ! python3 tools/check_win7_imports.py --architecture "$architecture" --d3d9 --json "$binary" > "$work/imports-client-win7-$architecture.json"; then
    cat "$work/imports-client-win7-$architecture.json" >&2
    exit 1
fi
out=${1:-"$work/artifacts"}
mkdir -p "$out"
out=$(cd -- "$out" && pwd)
stage="$out/package"
mkdir -p "$stage"
# Retire the previous package's external diagnostics files on an incremental build.
rm -f "$stage/horizon-ctl.exe" "$stage"/imports-control-win7-*.json
rustc "+$toolchain" -Vv > "$stage/build-info.txt"
printf '%s\n' 'profile=legacy opt-level=3 lto=thin codegen-units=1' >> "$stage/build-info.txt"
{ git rev-parse HEAD; "$cc" --version | head -1; printf '%s\n' "$target" "$RUSTFLAGS"; } >> "$stage/build-info.txt"
cp "$work/imports-client-win7-$architecture.json" "$stage/"
python3 - "$binary" "$repo/packaging/win7-client/TESTING.txt" "$out" "$architecture" <<'PY'
import hashlib, pathlib, shutil, sys, zipfile
binary, instructions, out = map(pathlib.Path, sys.argv[1:4])
architecture = sys.argv[4]
stage = out / 'package'
for source, name in [(binary, 'horizon-client.exe'), (instructions, 'TESTING.txt')]:
    shutil.copyfile(source, stage / name)
(stage / 'SHA256SUMS').write_text(''.join(
    f'{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n'
    for file in sorted(stage.iterdir()) if file.is_file() and file.name != 'SHA256SUMS'), encoding='utf-8')
archive = out / f'horizon-client-win7-{architecture}.zip'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as package:
    for file in sorted(stage.iterdir()):
        if file.is_file(): package.write(file, file.name)
(out / 'SHA256SUMS').write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n', encoding='utf-8')
PY
printf '%s\n' "Built $out/horizon-client-win7-$architecture.zip"
