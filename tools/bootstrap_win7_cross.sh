#!/usr/bin/env bash
# Download build tools only; no system installation and no files in the user's home.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
root=${1:-"$repo/work/win7/cross"}
architecture=${2:-${WIN7_ARCH:-all}}
if [[ "$architecture" == all ]]; then
    "$0" "$root" x64
    "$0" "$root" x86
    exit
fi
case "$architecture" in
    x64)
        prefix=x86_64-w64-mingw32
        architecture_packages='pool/main/b/binutils-mingw-w64/binutils-mingw-w64-x86-64_2.44-3+12+b1_amd64.deb b53f4a091b16a0d0952b8e3bd6027931641386258f96b37db1f0c62c787221d3
pool/main/g/gcc-mingw-w64/gcc-mingw-w64-x86-64-posix_14.2.0-19+27+b1_amd64.deb e32d24cfd1482394f16d9a84c1d0a9f5aa08ee826a79acc383c70cc8c23c4fe5
pool/main/g/gcc-mingw-w64/gcc-mingw-w64-x86-64-posix-runtime_14.2.0-19+27+b1_amd64.deb c898d96177574a8a7238564a1e799d8ddfed5a3d2ea005510305f9ad466a6f59
pool/main/m/mingw-w64/mingw-w64-x86-64-dev_12.0.0-5_all.deb 0bf89cf7454cccb49cd2a46a6f7b33896f6ae3a1a2fe33e02c40be6155bbb385'
        ;;
    x86)
        prefix=i686-w64-mingw32
        architecture_packages='pool/main/b/binutils-mingw-w64/binutils-mingw-w64-i686_2.44-3+12+b1_amd64.deb a8b90b43c53371e436e804a2335bfef77824dbd3d40c74e01acbed04ad333310
pool/main/g/gcc-mingw-w64/gcc-mingw-w64-i686-posix_14.2.0-19+27+b1_amd64.deb 31bbc7a215925deb972df1f5a8923e4b9ce3633903e0b30a908eb11dc82cdba6
pool/main/g/gcc-mingw-w64/gcc-mingw-w64-i686-posix-runtime_14.2.0-19+27+b1_amd64.deb 17a77024da685e0a56aac485afc518f9fba4d9ad557bce5d09ba2bf70c56bbb7
pool/main/m/mingw-w64/mingw-w64-i686-dev_12.0.0-5_all.deb ddd5f65c29014ae67b89df146a8016f38056a9d8eadbccd17cae4c00ccd01528'
        ;;
    *) echo 'Architecture must be x64 or x86.' >&2; exit 2 ;;
esac
mkdir -p "$root/downloads"
root=$(cd -- "$root" && pwd)
command -v dpkg-deb >/dev/null || { echo 'dpkg-deb is required (Debian/Ubuntu host).' >&2; exit 1; }
# Debian 13 amd64 packages. SHA-256 pins also protect a changed/misnamed mirror file.
# These binaries require glibc >= 2.38 and the ordinary GCC host libraries.
while read -r path digest; do
    file="$root/downloads/${path##*/}"
    if [[ ! -f "$file" ]] || ! echo "$digest  $file" | sha256sum --check --status; then
        curl --fail --location --retry 2 --output "$file.part" "https://deb.debian.org/debian/$path"
        echo "$digest  $file.part" | sha256sum --check --status
        mv -- "$file.part" "$file"
    fi
    dpkg-deb --extract "$file" "$root"
done <<PACKAGES
pool/main/g/gcc-mingw-w64/gcc-mingw-w64-base_14.2.0-19+27+b1_amd64.deb 8d2c64b886ab4a435a78ddc4cd3b510f149b65fbd6be8472e8f460627562718a
pool/main/m/mingw-w64/mingw-w64-common_12.0.0-5_all.deb 97dce5d0d8aff1cade4786ad4ef5078345477a598829afcac2e7f2fbc5399990
$architecture_packages
PACKAGES
# windres invokes the conventional GCC name when preprocessing embedded icons.
ln -sfn "$prefix-gcc-posix" "$root/usr/bin/$prefix-gcc"
"$root/usr/bin/$prefix-gcc-posix" --version
echo "Cross compiler extracted into $root (add $root/usr/bin to PATH)."
