# Horizon Launcher for Windows 7 x64: implementation status

This is an experimental implementation report, **not completed Windows 7 support**.
The required minimum is Windows 7 SP1 x64 on Atom N455/GMA 3150 with 2 GB RAM.
Radeon HD 4570 needs a separate physical-system run. The user made x86 optional;
no 32-bit build or compatibility claim is included.

## Source baseline and scope

Work is in `Mamsikas/horizon-legacy`. The checked HEAD before edits was
`59aa0e53df43fe087849250c7fd91de3b8636f2e`. The specification's reference,
`36fc4fa187a4e1f3664c8891dc91e2c9e5a5b911`, exists but was not this checkout's HEAD.
The intervening launcher changes were in `backend.rs`, `main.rs`, and `nightly.rs`;
line numbers from the specification were not treated as a contract.

The modern `launcher/src/main.rs` and GPUI pages remain intact. `gui` stays the default
feature; the separate native binary uses `--no-default-features --features win7`.
Root README/roadmap, game protocol, world behavior and global graphics defaults are unchanged.
The fork later advanced main to `9319ad4bf1c2271662f2caa52b1478c0a1bf2617`; those game-only
changes do not overlap the launcher/preparation files changed here.
The only protocol-crate change disables unused `ipconfig/computer` bindings; adapter DNS
resolution is unchanged. The effective `serde_json/preserve_order` feature previously
supplied by GPUI is explicit so the core-only build keeps the Forge planner's behavior.

USER32 controls and GDI text replace the incompatible presentation layer. Forms come
from the existing clap command definitions and invoke the same CLI/core in background
workers. Shared wardrobe/shop/storage services supply the additional native actions.
A form's presence is not proof that its complete scenario passed on Windows 7.

## L0 audit matrix

| Subsystem | Modules inspected | Win7/GMA limitation | Solution and remaining work |
|---|---|---|---|
| Modern UI | `launcher/Cargo.toml`, `main.rs`, pages; gpui-fast-windows 0.1.13 | D3D11 FL 11_1/11_0/10_1, compute, CreateDXGIFactory2, IDXGIFactory6, newer text/composition APIs | Exclude GPUI from the legacy graph; separate USER32/GDI entrypoint |
| Core/UI coupling | `lib.rs`, `cli.rs`, `pictures.rs` | Mandatory GUI dependencies; Linux GPU texture-limit probe | Optional GUI dependencies and required-features; probe only under `gui` |
| Authentication | `account.rs`, `account/session.rs`, `microsoft.rs` | Real Device Code, TLS, token storage and ownership must work; Azure app configuration is required | Reuse SignIn/token store/owner gate; client ID is currently absent; no bypass |
| Versions/architecture | `backend::{PLATFORMS,OWN_ASSETS,ASSET,LAUNCHER_ASSET,GameArchitecture}` | Modern x64 archives are not Win7 builds; x86 was absent | Separate signed legacy names, no modern fallback, hash-bound client identity |
| Resource preparation | `sources.rs`, `mojang.rs`, `backend::preparer`, `horizon-prepare` | Vanilla exporters/data generation require Java 25 even with compiled exporter classes | Pinned official Liberica JRE/JDK candidate; retain license/input/hash checks; fresh install unproved |
| Preparation memory | `horizon-prepare/{java,worker,pool}.rs` | Persistent workers request 2 GiB and several JVMs can coexist | Native workers opt into one job and a preparation-only 512 MiB cap; sufficiency unproved |
| Launch/processes | `backend::{client_as,client_command,spawn}`, `instances.rs`, `launch.rs`, `windows_runtime.rs` | Win7 ABI, exact resources/home/session, stable runtime identity | Shared command/session pipeline; child-only legacy graphics selection; mock contract probe |
| Mods/packs | `packs.rs`, `modpacks.rs`, `browser/*`, `native_mods.rs`, `rust_toolchain.rs` | Native mods rebuild the entire client with a modern host toolchain | Content operations remain available; unsupported native build/Rust installation fails explicitly |
| Shell/files | `shortcut.rs`, `desktop_seams.rs`, `installer.rs` | Transitive WinRT/combase imports; recoverable deletion and path handling | Raw ole32/shell32 helpers; long/UNC recycle paths fail without deleting data |
| Storage/update | `data_location.rs`, `relocation.rs`, `backend.rs`, `update_handoff.rs`, `update_recovery.rs` | PowerShell 3 is absent on stock Win7; replacement races and locked EXEs | Toolhelp32 migration guard, exclusive install/recovery lease, staged rollback; physical update tests pending |
| Rust/CPU | std, getrandom, Windows bindings, release workflows | Current pc-windows targets do not guarantee NT 6.1; N455 lacks SSE4/POPCNT/AVX | Pinned Tier 3 Win7 target with build-std and x86-64/SSE2 baseline; imports and hardware both need checking |

The first linked binary exposed real transitive WinRT/combase imports. Removing GPUI
alone was insufficient: unused ipconfig computer/registry bindings were removed and
Shell COM calls use ole32 directly. No dependency implementation was copied. Runtime
vendor documentation, binary metadata and imports were inspected; Wine/JVM source was not used.

## Functional parity

[win7-action-parity.md](win7-action-parity.md) records all **108 forms** individually.
Both physical Win7 targets remain **NOT TESTED** for every action, regardless of host tests.

| Modern page | Native operations through shared core | Remaining limitations |
|---|---|---|
| Play/Home | `play`, `prepare`, account selection/refresh/sign-out, Device Code panel, instances and logs | Real signed Win7 game/session launch and full Home overview are unverified |
| Versions | `versions`, `install`, `remove`, `import-build`, `update`, selected version settings | Signed legacy releases must be published; an imported modern binary cannot inherit legacy identity |
| Mods | `mods`, `packs`, `modpack`, visible native-build action | No visual cards/previews; native compilation is explicitly unsupported |
| Servers | `servers`, `server`, `multiplayer`, `play --server/--join`, worker stdin input | Real server/client lifecycle and console input need compatible binaries and Win7 tests |
| Wardrobe | `legacy wardrobe-list/equip/unequip/visible/preferences/claim`; offline skins/capes | Shared auth/service/offline queue used; real network flow unverified; no 3D preview |
| Shop | `legacy shop-catalogue/history/quote/checkout/claim` | Explicit Confirm and validated external HTTPS checkout URL; no live purchase was performed; no visual try-on |
| Notes | `notes` with optional tag | Text output instead of rich GPUI presentation |
| Settings | `settings`, `game-args`, `no-ai`, `native-cache`, `rust`, `java`, `data`, `links`, storage status/move/retry/default | Appearance/background preferences remain modern-UI settings; physical migration/update acceptance pending |

Instance create/select/edit/delete/import/export/other-launcher copy, world backup/copy/delete,
data packs, resource/shader packs, mod browsing, screenshots and logs use their existing
CLI/core operations. Paths are entered in native fields; GPUI gallery previews and drag-and-drop
are not reproduced. Existing checks and settings formats remain shared.

The form catalog excludes MCP/completions/agent configuration, the modern GUI benchmark,
secret session-file output and CLI sign-in. A dedicated panel handles sign-in instead.
The native wardrobe/shop/storage extension is not exposed as generated MCP tools.
Storage handoff waits for the worker to finish, exits the window, then delegates to the
existing relocation helper. On Win7 its process check conservatively blocks any active
Java/Cargo/Rust build tool because Toolhelp32 does not provide its command line.

## Preparation memory policy

There was **no launcher-level RAM-aware heap selector**. Persistent exporter workers used
an explicit `-Xmx2G`. Java commands without an explicit heap used JVM ergonomics; parallel
job count was selected from logical processors, capped at eight.

Only native UI workers now supply these defaults when the corresponding variable is absent:

```text
HORIZON_PREPARE_JOBS=1
HORIZON_PREPARE_MAX_HEAP_MIB=512
```

A user-supplied value wins. The cap affects preparation JVMs, not game heap or global game
options. Manual CLI preparation is not implicitly assigned this native-worker profile.
512 MiB is a conservative candidate for the 2 GB machine, **not a measured minimum**:
a full vanilla exporter run can still expose an OutOfMemoryError and require adjustment.
The modern launcher's arguments remain unchanged unless the heap-cap environment option is set.

## Build, artifacts and trust

See [TESTING.md](../packaging/win7/TESTING.md). On the documented Linux build host:

```sh
bash tools/bootstrap_win7_cross.sh
bash tools/build_win7_launcher.sh
```

The separate build pins `nightly-2026-11-01`, `x86_64-win7-windows-gnu`,
`-Z build-std=std,panic_abort`, `-C target-cpu=x86-64`, static CRT and subsystem 6.1.
SHA-pinned MinGW packages are extracted locally. windows-targets 0.48.5 needs its
Cargo-verified import-library path supplied explicitly because it predates the win7 vendor target.
The main workspace toolchain is not downgraded.

Outputs are `horizon-launcher-win7-x64.zip`, the separate
`horizon-win7-process-test-x64.zip`, PE reports, `build.txt`, and `SHA256SUMS.local`.
The latter is an **unsigned local checksum list**, not a release signature.
The ZIPs contain no Mojang jars/assets/prepared resources or account secrets.

Legacy build-time `HORIZON_WIN7_RELEASE_REPO=owner/repository` and
`HORIZON_WIN7_RELEASE_PUBLIC_KEYS` select an explicit fork feed and base64 Ed25519 keys.
A custom feed without valid keys fails closed. Modern feeds/keys remain unchanged.
The configuration mechanism does not publish or sign releases itself.

## Recorded validation and its limits

| Check | Result | Limit |
|---|---|---|
| Formatting and Clippy core/native with `-D warnings` | PASS | Does not prove Windows runtime behavior |
| Core-only library tests | 653 passed, 7 ignored | Host tests; ignored cases remain unverified |
| Native entrypoint tests | 35 passed | Parser/worker/core contracts, not physical GUI acceptance |
| Java preparation / routing / Rust routing | 5 / 9 / 17 passed | Does not run the full licensed vanilla exporter pipeline |
| Resolver / PE-audit tests / translation completeness | 7 / 10 passed / PASS | Resolver algorithm retained; import checker is static |
| Both release EXEs and strict PE audit | PASS, x64, subsystem 6.1 | No GPUI/wgpu runtime or unconditional D3D/DXGI/Vulkan/WinRT/combase imports; late binding and CPU dispatch still need hardware tests |
| Wine 10, NT 6.1.7601 profile, Xvfb 1024x600 | Native window, mouse/Tab/scroll, installed versions, Unicode instance persistence, worker completion/re-enabled controls: PASS | Wine is not Windows 7 or either target GPU |
| Missing Microsoft client ID | Native button safely refused sign-in | Actual OAuth/ownership/refresh is untested |
| Mock process with Unicode/spaces | resources/home/cwd/inherited env/child renderer: PASS | Not an authorized game launch or real game-session test |
| Liberica 25 and cached managed-runtime helper | Version probe and checked archive extraction/publish: PASS in ASCII location under Wine | No network transfer or Minecraft exporter run |

A vendor JVM bootstrap failed in a Unicode directory under Wine (`could not find java.dll`).
The same runtime failed there when invoked directly. The helper preserved its archive and
refused to publish it. Runtime storage now uses the core's existing ASCII tool directory.
The shipping helper repeated the same test from a Unicode launcher home and passed through
the ASCII ProgramData runtime path. Java discovery found the installed runtime; unsupported
Rust installation was refused before any download. The local smoke protocol records exact
EXE hashes in `packaging/win7/LOCAL_VALIDATION.json`. This remains a Wine result.

| Physical target | GUI/input/DPI/Aero-off | Mock/game/fresh install | Startup/idle CPU/RSS/peak |
|---|---|---|---|
| Win7 SP1 x64, N455/GMA 3150, 2 GB, Intel driver | NOT TESTED | NOT TESTED | NOT MEASURED |
| Win7 SP1 x64, Radeon HD 4570, AMD driver | NOT TESTED | NOT TESTED | NOT MEASURED |
| Win7 x86 | Not built; optional target omitted | Not claimed | Not measured |
| Modern Win10/11/Linux/macOS GPUI launcher | UI source preserved; full runtime regression still required | Not established by legacy checks | Not measured |

The fork's inherited checks select self-hosted runners that are unavailable here. The separate
legacy workflow therefore also runs the existing default GPUI launcher formatting, Clippy
and unit-test commands on a GitHub-hosted Linux runner. Its current result must be read
from Actions; local core checks are not a substitute for that modern build.

## Unmet criteria and stage status

| Stage | Demonstrated work | Remaining acceptance |
|---|---|---|
| L0 | Source/dependency audit and both EXE static import checks | Late-bound API and physical-system validation |
| L1 | Actual native window and mock contract passed Wine smoke | Both physical Win7 targets |
| L2 | Shared core behind 108 forms, auth state, logs and extension actions | Per-action Win7 tests; real network/account/client scenarios; visual gaps recorded above |
| L3 | Pinned JVM candidate, staged verification/rollback, explicit memory profile | Actual Win7 java/javac and licensed autonomous fresh install without a modern PC |
| L4 | Separate ZIPs, signed-asset policy, update recovery and fork trust configuration | Published signed legacy releases and interrupted update/rollback on Win7 |
| L5 | Not completed | N455/GMA 3150, HD 4570 and modern GUI runtime regressions |

The user confirmed no `HORIZON_MSA_CLIENT_ID` is available. Real sign-in and owner-gated
launch remain blocked until an eligible Azure app is configured. Native mods rebuild the
whole client with a modern compiler; a Win7 target/ABI/cache or implemented delegated
builder is still required. Unsupported modern Rust installation is refused before download.
The game binary itself remains separate work; a mock is not evidence that it runs.

Cancellation is cooperative for Device Code and signed archive transfers/commit boundaries;
custom browser/modpack transfers and exporter loops do not yet have full cancellation.
Checked Java archives survive failed extraction/probes, but generic HTTP Range byte-resume
is not implemented. Long/extended/UNC recycle paths are rejected with files retained.
The legacy entrypoint deliberately does not invoke the shared quiet installer registration;
distribution is ZIP-only and the native graphical installer remains unimplemented.

Use fresh folders and the procedure in TESTING.md. On each physical system record build 7601,
installed KBs, GPU driver, privileges, resolution/DPI/Aero state, startup and memory measurements.
For each action record PASS/FAIL/NOT TESTED and its log. Wrong hashes/signatures, invalid
credentials, no network, cancellation, low disk, Unicode/long paths, interrupted downloads,
forced-update termination and rollback still need the specified target-system checks.

## Changed files and purpose

| Exact file(s) | Required function or reason |
|---|---|
| `launcher/Cargo.toml` | Optional GUI/core graph, native binaries, explicit preserved JSON order |
| `launcher/Cargo.lock`, `Cargo.lock` | Minimal dependency graph changes for compatible imports |
| `launcher/build.rs` | Rebuild when legacy release feed/public-key configuration changes |
| `launcher/src/lib.rs` | Connect isolated native helpers while retaining shared core |
| `launcher/src/bin/horizon-launcher-win7.rs` | Native startup, CLI/probes, recovery and error handling |
| `launcher/src/bin/mock-horizon-client.rs` | Independent process-contract fixture, not a game or auth bypass |
| `launcher/src/win7/mod.rs` | Native module boundary |
| `launcher/src/win7/catalog.rs` | Forms from existing CLI, typed validation, Unicode/default/repeated arguments |
| `launcher/src/win7/window.rs` | USER32/GDI controls, focus/scroll/DPI, sign-in state, clipboard, console and storage handoff |
| `launcher/src/win7/worker.rs` | Background CLI execution, bounded output, cancellation and preparation profile |
| `launcher/src/win7/smoke.rs` | Mock launch through shared low-level command contract |
| `launcher/src/backend.rs` | Legacy selection/identity, launch env, staged install/recovery/lease, cancellation and Java routing |
| `launcher/src/backend/win7_tests.rs` | Observable legacy selection, signed staging, ownership-independent command and recovery contracts |
| `launcher/src/signed.rs` | Explicit compile-time legacy feed with mandatory signature verification |
| `launcher/src/cli.rs` | Pipe progress and native extension routing through shared core |
| `launcher/src/cli_tools.rs` | Keep native checkout/storage out of agent tools and preserve modern command reference |
| `launcher/src/win7_actions.rs` | Existing wardrobe/shop/storage services, confirmation and relocation handoff |
| `launcher/src/java.rs` | Route Java 25 install/discovery through the managed legacy runtime |
| `launcher/src/legacy_java.rs` | Pinned official JVM download, hash/layout/version checks, ASCII tool path, staging/rollback |
| `launcher/src/native_mods.rs` | Explicit unsupported legacy native-build diagnostic |
| `launcher/src/rust_toolchain.rs` | Refuse unsupported modern host compiler before download on Win7 |
| `launcher/src/pictures.rs` | Gate GPU probing behind the GUI feature |
| `launcher/src/win7_shell.rs` | Win7 ole32/shell32 links and recoverable deletion without WinRT wrappers |
| `launcher/src/shortcut.rs` | Route non-GUI Windows links to the compatible shell helper |
| `launcher/src/desktop_seams.rs` | Use the compatible recycle-bin operation |
| `launcher/src/installer.rs` | Exercise shell-link target checks without modern COM wrappers |
| `launcher/src/relocation.rs`, `launcher/src/relocation/win7.rs` | Win7 Toolhelp32 process guard without PowerShell 3 |
| `launcher/src/relocation/tests.rs` | Verify relocation refuses active games/build tools |
| `crates/horizon-prepare/src/java.rs`, `crates/horizon-prepare/src/worker.rs` | Preparation-only opt-in heap cap, preserving modern defaults |
| `crates/horizon-protocol/Cargo.toml` | Drop unused ipconfig computer/registry feature without changing DNS behavior |
| `tools/bootstrap_win7_cross.sh` | SHA-pinned locally extracted cross tools |
| `tools/build_win7_launcher.sh` | Pinned target/std/CPU, graph/import gates, provenance and ZIPs |
| `tools/check_win7_imports.py`, `tests/test_check_win7_imports.py` | Fail-closed normal/delay PE import checker and rejection fixtures |
| `packaging/win7/LOCAL_VALIDATION.json` | Exact local-binary Wine evidence and explicit untested hardware scenarios |
| `.github/workflows/launcher-win7.yml` | Separate experimental build/artifact workflow |
| `packaging/win7/TESTING.md`, `packaging/win7/API_BASELINE.md` | Reproducible build/hardware procedure and documented API baseline |
| `docs/win7-launcher-status.md`, `docs/win7-prepare-audit.md`, `docs/win7-action-parity.md` | Audit, evidence, per-action status and unmet criteria |
| `launcher/lang/*.json` below | Native UI/diagnostics in every existing locale table; established English fallbacks where untranslated |

## Exact locale-file appendix

Each of these 86 files has the same purpose described above:

```text
launcher/lang/af.json
launcher/lang/ar.json
launcher/lang/az.json
launcher/lang/ba.json
launcher/lang/be.json
launcher/lang/bg.json
launcher/lang/br.json
launcher/lang/bs.json
launcher/lang/ca.json
launcher/lang/cs.json
launcher/lang/cv.json
launcher/lang/cy.json
launcher/lang/da.json
launcher/lang/de.json
launcher/lang/el.json
launcher/lang/eo.json
launcher/lang/es.json
launcher/lang/et.json
launcher/lang/eu.json
launcher/lang/fa.json
launcher/lang/fi.json
launcher/lang/fil.json
launcher/lang/fo.json
launcher/lang/fr.json
launcher/lang/fur.json
launcher/lang/fy.json
launcher/lang/ga.json
launcher/lang/gd.json
launcher/lang/gl.json
launcher/lang/haw.json
launcher/lang/he.json
launcher/lang/hi.json
launcher/lang/hr.json
launcher/lang/hu.json
launcher/lang/hy.json
launcher/lang/id.json
launcher/lang/ig.json
launcher/lang/is.json
launcher/lang/it.json
launcher/lang/ja.json
launcher/lang/ka.json
launcher/lang/kk.json
launcher/lang/kn.json
launcher/lang/ko.json
launcher/lang/ky.json
launcher/lang/la.json
launcher/lang/lb.json
launcher/lang/li.json
launcher/lang/lmo.json
launcher/lang/lo.json
launcher/lang/lt.json
launcher/lang/lv.json
launcher/lang/mk.json
launcher/lang/mn.json
launcher/lang/ms.json
launcher/lang/mt.json
launcher/lang/nl.json
launcher/lang/no.json
launcher/lang/oc.json
launcher/lang/pl.json
launcher/lang/pt-br.json
launcher/lang/pt.json
launcher/lang/ro.json
launcher/lang/ru.json
launcher/lang/sah.json
launcher/lang/se.json
launcher/lang/sk.json
launcher/lang/sl.json
launcher/lang/so.json
launcher/lang/sq.json
launcher/lang/sr.json
launcher/lang/sv.json
launcher/lang/szl.json
launcher/lang/ta.json
launcher/lang/th.json
launcher/lang/tr.json
launcher/lang/tt.json
launcher/lang/uk.json
launcher/lang/ur.json
launcher/lang/uz.json
launcher/lang/vec.json
launcher/lang/vi.json
launcher/lang/yi.json
launcher/lang/yo.json
launcher/lang/zh.json
launcher/lang/zu.json
```
