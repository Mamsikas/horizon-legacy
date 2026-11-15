# Building the native launcher

The launcher has two editions using the same native controls and launch contract:

| Archive | Minimum OS | Resource setup | Client |
|---|---|---|---|
| `horizon-launcher-win7-x64.zip` | Windows 7 SP1 x64 | Prepare locally from the user's pinned Minecraft 26.3 files | Selected, bundled or signed official release |
| `horizon-launcher-win7-x86.zip` | Windows 7 SP1 x86 | Select an already prepared Horizon installation | Selected, installed or bundled x86 executable; no downloads |

Both ZIPs contain only the launcher EXE and an English player guide. The modern launcher and the separate client branch are unchanged.

## Reproducible cross build

On a Debian/Ubuntu x64 build host with rustup, Python 3, curl, ripgrep and `dpkg-deb`:

```sh
export WIN7_BUILD_ROOT="$PWD/work/win7"
export WIN7_CROSS_ROOT="$WIN7_BUILD_ROOT/cross"
export CARGO_HOME="$WIN7_BUILD_ROOT/cargo"
export RUSTUP_HOME="$WIN7_BUILD_ROOT/rustup"
# Install rustup in these directories first, or use an existing CARGO_HOME/RUSTUP_HOME.
tools/bootstrap_win7_cross.sh "$WIN7_CROSS_ROOT"
tools/build_win7_launcher.sh
```

The scripts pin the cross compiler packages and Rust nightly. They build `std` separately for the official Tier 3 [`*-win7-windows-gnu` targets](https://doc.rust-lang.org/rustc/platform-support/win7-windows-gnu.html). `WIN7_ARCH=x64` or `WIN7_ARCH=x86` selects one edition; the default builds both. CI runs one matrix job per architecture. Each artifact must pass its own PE32/PE32+ and Win7 import audit before packaging. SSE2 is the CPU baseline, without SSE4, POPCNT or AVX requirements.

## Java and Windows XP

The x64 preparer retains its working pinned Java 25 runtime. The x86 launcher never installs or starts Java and does not bundle exporter sources. It reads prepared resources through the existing core checker without changing them.

Installing Java 24 or 8 cannot directly run the pinned Minecraft 26.3 exporters: they load the game's Java 25 classes. Windows x86 was removed from OpenJDK in [JDK 24 (JEP 479)](https://openjdk.org/jeps/479); current [Liberica 25 releases](https://github.com/bell-sw/Liberica/releases/tag/25.0.5.0.3%2B5) have no Windows i586 package. The researched [OpenJDK 25 XP backport](https://github.com/mgpeter/fork-openjdk-25-windows-xp-x64) explicitly supports x64 only. [JvmDowngrader](https://github.com/unimined/JvmDowngrader/releases/tag/2.0.0) transforms class files and APIs; adopting it would require validating every exporter and generated result against the original preparation. It is not used or represented as verified here.

The ready-installation route removes Java as a dependency, but does not make this launcher XP compatible. The Rust standard library and reachable dependencies still import Vista/Win7 APIs, including SRW locks, condition variables, known-folder discovery and BCrypt. Lowering the PE subsystem version cannot replace them. [Rust9x](https://github.com/rust9x/docs) ports the standard library, but its documentation explicitly requires separate work on crates that call Windows APIs directly. This build therefore targets Windows 7 SP1; XP is not advertised or packaged as supported.

Physical Windows 7/XP execution is not established by compilation, a static audit or Wine. Game architecture, rendering support and hardware validation belong to the selected client; this launcher does not port the game.
