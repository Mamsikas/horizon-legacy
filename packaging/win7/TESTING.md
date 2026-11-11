# Windows 7 x64 launcher validation

This is an experimental, separate native launcher. A successful build and PE
import audit do not establish Windows 7 support. Do not substitute the modern
`horizon-windows-x64.zip` game archive for a signed `win7-x64` client.

## Build on Linux

Use an amd64 Debian 13 / Ubuntu 24.04 or newer host with Rustup, curl, dpkg-deb,
Python 3, ripgrep, and GCC host libraries (`libgmp10`, `libisl23`, `libmpc3`,
`libmpfr6`, `libzstd1`, `zlib1g`). The bootstrap extracts SHA-256-pinned Debian
MinGW packages locally. It requires glibc 2.38 or newer, and never installs them
on the host. Use a dedicated build folder; these are development tools, not
software the Windows user must install.

```sh
bash tools/bootstrap_win7_cross.sh
bash tools/build_win7_launcher.sh
```

Rust and its rebuilt standard library are pinned to `nightly-2026-11-01`, target
`x86_64-win7-windows-gnu`. CPU baseline is `x86-64`/SSE2; the CRT is static.
The scripts use `work/win7/` unless explicit environment paths are supplied.
`CARGO_HOME`, `RUSTUP_HOME`, `WIN7_BUILD_ROOT`, `WIN7_CROSS_ROOT`, and `WIN7_CC`
can select existing tools. No global Rust downgrade is needed.

`horizon-launcher-win7-x64.zip` contains no Minecraft files. `SHA256SUMS.local`
is a local checksum list, **not** a release signature. Published updates must
retain Horizon's signed-checksum verification. Import checking rejects unknown
DLLs/APIs and modern graphics dependencies, but cannot inspect dynamically
resolved calls, CPU-dispatch correctness, or behavior on real hardware.

## Required hardware checks

Test Windows 7 SP1 build 7601 x64 on Atom N455/GMA3150 with the Intel driver and
2 GB RAM, and separately on Radeon HD4570. Record OS updates, driver version,
RAM, startup time, working set, and peak memory. Also test with Aero disabled.
An x64 program cannot run on 32-bit Windows; no x86 build is claimed here.

Use empty scratch paths for launcher data and game data. Do not point tests at
real worlds or accounts without backing them up. Preserve the logs, but redact
tokens, session JSON, device codes, and authorization headers before sharing.

1. Extract the ZIP to a writable folder containing spaces and Cyrillic text.
   Start the executable without administrator rights. Verify keyboard focus,
   Tab navigation, mouse, scrolling, clipboard, text input, DPI, and resize.
2. Extract `horizon-win7-process-test-x64.zip` next to the launcher, then run
   the following from Command Prompt, substituting a new empty home path:

   ```bat
   set HORIZON_SMOKE_INHERITED=legacy-contract
   horizon-launcher-win7.exe --smoke-client "%CD%\mock-horizon-client.exe" "%TEMP%\Horizon Smoke 12345" "%CD%\resources test"
   type "%TEMP%\Horizon Smoke 12345\mock-launch.json"
   ```

   Inspect the JSON and confirm the exact
   resource path, instance home/game directory, inherited environment, and
   legacy graphics selector. A mock does not prove that the game itself works.
3. Exercise every action in the functional parity report. Record PASS, FAIL,
   or NOT TESTED per action, with the actual error and relevant log path.
4. With an owned Java Edition Microsoft account, test device-code sign-in,
   cancel/expiry, token refresh, account switching, and ownership rejection.
   Confirm no secret is in the process command line or logs.
5. Attempt a fresh installation into an empty folder on Win7 itself. Record
   whether resource preparation requires JDK 25. Importing resources prepared
   on another OS does not satisfy independent first installation.
6. Interrupt and resume downloads; test corrupted hashes/signatures, invalid
   credentials, no network, low disk space, and long/Cyrillic paths. Verify
   recovery preserves the previous working installation and player data.
7. Test a staged launcher update, forced termination, restart, and rollback.
   Confirm only the matching legacy architecture is selected and verified.
8. Install/build native mods using the selected supported toolchain. Missing
   Win7 toolchain/ABI support remains an unmet criterion, not an implicit pass.

Until the two physical systems and the licensed fresh-install path have been
tested, those acceptance criteria remain **NOT TESTED / INCOMPLETE**.
