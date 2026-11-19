#!/usr/bin/env bash
# A separate Tier 3 target and std; never lower the modern launcher's toolchain.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
toolchain=nightly-2026-11-01
arch=${WIN7_ARCH:-all}
case "$arch" in
    all) architectures=(x64 x86) ;;
    x64|x86) architectures=("$arch") ;;
    *) echo 'WIN7_ARCH must be all, x64 or x86.' >&2; exit 1 ;;
esac
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
command -v rustup >/dev/null || { echo "Install rustup with CARGO_HOME=$CARGO_HOME RUSTUP_HOME=$RUSTUP_HOME first." >&2; exit 1; }
rustup toolchain install "$toolchain" --profile minimal --component rust-src
# The source packager must follow helper dependencies before producing a player archive.
rustc "+$toolchain" --edition=2021 --test "$repo/launcher/build_support.rs" -o "$work/source-bundle-check"
"$work/source-bundle-check"
if [[ "$arch" != x64 ]]; then
    rustc "+$toolchain" --edition=2021 --test "$repo/launcher/src/win7/resource_search.rs" -o "$work/resource-search-check"
    "$work/resource-search-check"
fi
cd -- "$repo"
out=${1:-"$work/artifacts"}
mkdir -p "$out"
out=$(cd -- "$out" && pwd)
for arch in "${architectures[@]}"; do
    if [[ "$arch" == x64 ]]; then
        target=x86_64-win7-windows-gnu
        prefix=x86_64-w64-mingw32
        cpu=x86-64
    else
        target=i686-win7-windows-gnu
        prefix=i686-w64-mingw32
        cpu=pentium4
    fi
    cc=${WIN7_CC:-"$cross/usr/bin/$prefix-gcc-posix"}
    [[ -x "$cc" ]] || { echo "Run tools/bootstrap_win7_cross.sh '$cross' first (or set WIN7_CC)." >&2; exit 1; }
    target_key=${target//-/_}
    export "CARGO_TARGET_${target_key^^}_LINKER=$cc"
    export "CC_$target_key=$cc"
    export "AR_$target_key=$prefix-ar"
    # SSE2 baseline on both targets. Never inherit target-cpu=native or newer CPU features.
    unset CARGO_ENCODED_RUSTFLAGS
    export RUSTFLAGS="-C target-cpu=$cpu -C target-feature=+crt-static -C panic=abort -C link-arg=-Wl,--major-subsystem-version,6 -C link-arg=-Wl,--minor-subsystem-version,1"
cargo "+$toolchain" tree --locked --manifest-path launcher/Cargo.toml --target "$target" \
    --no-default-features --features win7 --edges normal > "$work/dependency-tree-$arch.txt"
if rg -qi '(^|[^[:alnum:]_])(gpui|gpui-fast|wgpu|accesskit_windows)([^[:alnum:]_]|$)' "$work/dependency-tree-$arch.txt"; then
    echo 'A GPU/modern UI dependency reached the legacy build; see dependency-tree.txt.' >&2
    exit 1
fi
# windows-targets 0.48.5 predates the win7 vendor targets: its import-library build script
# recognizes only pc/uwp and omits this search path. Use Cargo's checksum-verified package,
# without patching the registry or rebuilding modern targets with different flags.
metadata="$work/cargo-metadata-$arch.json"
cargo "+$toolchain" metadata --locked --manifest-path launcher/Cargo.toml \
    --format-version 1 --filter-platform "$target" --no-default-features --features win7 > "$metadata"
import_lib=$(python3 - "$metadata" "$target" <<'PY'
import json
import pathlib
import sys

packages = json.loads(pathlib.Path(sys.argv[1]).read_text())['packages']
package_name = 'windows_i686_gnu' if sys.argv[2].startswith('i686') else 'windows_x86_64_gnu'
package = next(p for p in packages
               if p['name'] == package_name and p['version'] == '0.48.5')
directory = pathlib.Path(package['manifest_path']).parent / 'lib'
if not (directory / 'libwindows.0.48.5.a').is_file():
    raise SystemExit(f'Cargo did not provide {package_name} 0.48.5 import libraries')
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
if ! python3 tools/check_win7_imports.py --architecture "$arch" --json "$binary" > "$work/imports-win7-$arch.json"; then
    cat "$work/imports-win7-$arch.json" >&2
    exit 1
fi
readme="$repo/packaging/win7/README.md"
[[ "$arch" == x86 ]] && readme="$repo/packaging/win7/README-x86.md"
python3 - "$binary" "$readme" "$out" "$arch" <<'PACK'
import hashlib
import pathlib
import sys
import zipfile

binary, readme, out = map(pathlib.Path, sys.argv[1:4])
archive = out / f'horizon-launcher-win7-{sys.argv[4]}.zip'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as package:
    package.write(binary, 'horizon-launcher-win7.exe')
    package.write(readme, 'README.md')
(out / 'SHA256SUMS').write_text(
    ''.join(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n'
            for path in sorted(out.glob('horizon-launcher-win7-*.zip'))), encoding='utf-8')
PACK
printf '%s\n' "Built $out/horizon-launcher-win7-$arch.zip"
done
