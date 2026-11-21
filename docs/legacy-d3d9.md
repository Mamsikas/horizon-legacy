# Horizon DX9: maintenance and handoff

This fork provides one native Minecraft Java Edition 26.3 client using Direct3D 9,
primarily for Atom N455 / Intel GMA3150 and Windows 7. It is a renderer/backend port,
not a different game implementation. Preserve vanilla gameplay, protocol, resources,
lighting inputs and player choices. Deep performance work is the next task (TZ 3).

## Repository and upstream

- `main` is the DX9 distribution branch. Develop on `dx9-dev` or a focused branch,
  then open PRs against this repository's `main`, not the official Horizon repository.
- Upstream is `https://github.com/horizon-rust/horizon`. This integration uses
  `df50a07e73` (2026-11-18). It includes the fork's synced upstream `bc07dd266c`.
  Record the exact upstream revision whenever updating; do not chase a moving head
  during validation. Merge history, never replace it with a force-push.
- `archive/dx9-before-nov18` preserves the last pre-integration source, `42f0b42d47`.
  Previous measurements and binaries refer to that source, not the new LTO build.
- After an upstream merge, resolve moved code in its new home. Remove redundant
  fork fixes once the equivalent upstream behavior exists. The November integration
  removes duplicate Windows process CPU/RSS sampling in favor of `horizon-system`.
  The system-wide Windows CPU probe remains needed. The offline encrypted-login
  fix remains separate: an offline profile sends the encryption response and the
  server decides admission; signed-in session authentication is unchanged.
- Keep this document and the fork-specific instructions in `AGENTS.md` current.
  Upstream's other docs describe upstream features, not a promise that DX9 supports
  every GPU pipeline. Do not publish official Horizon releases from this fork.

## Requirements and defaults

- Windows 7 SP1 and a real Direct3D 9 HAL driver. REF/WARP or CPU rasterization
  is not selected. Software *vertex processing* still rasterizes on the GPU.
- x64 permits SSE3/SSSE3, matching N455; x86 requires SSE3 and is large-address-aware.
  Do not use host-native SSE4, POPCNT or AVX. K8 with SSE3 need not have SSSE3.
- The Windows D3D9 runtime is required; an external shader compiler/DXC is not.
  Basic geometry uses FFP with two texture stages. Full environmental/distance fog
  uses embedded PS2 when available, with FFP fallback. Hardware VS2 enables smaller
  vertices but is optional: GMA3150 has no hardware T&L and uses software VP.
- The GPU's shader capability and a historical DX9b runtime are different questions.
  Native XP / historical-runtime compatibility is not established. The owner ran
  x86 on XP with OneCoreAPI, with mouse capture and long-running chunk/crash issues.
  Both packages still use the Win7 ABI and PE subsystem 6.1; do not call them XP builds.
- Prepared legal Horizon resources for Minecraft 26.3 are supplied by the player.
  Never commit or ship Mojang assets, jars, decompiled sources, resources or saves.
- Default initial window: 854x480; explicit size/fullscreen wins. Prefer RGB565 with
  checked 32-bit fallback. D24 depth takes priority over 16-bit color. Exclusive
  fullscreen is not required. Textures/alpha stay 32-bit; RGB565 can introduce banding.
- VP Auto/Hardware/Software and Color Depth 16/32 are in Graphics > General >
  Direct3D 9; they persist and require restart. A developer environment VP override
  takes precedence. F3 reports effective VP/color, startup logs identify the adapter.
- No forced low preset, distance reduction, FPS cap or fixed 128 MiB cap. Advisory
  budgets follow driver/system memory, with bounded paging/fallback under pressure.
  Ordinary tests should remove old HORIZON_D3D9_MESH_MIB/TEXTURE_MIB overrides.
  New upstream guest-capable threads reserve 136 MiB of virtual stack each; this
  is address space, not committed RSS. Keep that distinction in x86 memory reports.

## Ownership and source map

| Area | Files / contract |
| --- | --- |
| Native Windows/D3D9 facade | `crates/horizon-client-d3d9/src/`: COM, pointers, locked memory and Win32 stay here. Device/resources are !Send/!Sync and retain their owner. |
| Backend selection | `horizon-client-render/src/renderer.rs`: compile-time DX9 alias on Windows + legacy-d3d9. Shared gfx Frame/types remain usable without creating a wgpu device. |
| World renderer | `renderer/legacy.rs`, `legacy/frame.rs`, `legacy/compat.rs`: Frame consumption, lighting, state, unsupported capabilities. |
| Geometry transfer | `legacy/mesh.rs`, `mesh/{convert,upload,arrival,compact,shadows}.rs`, `solid_cache.rs`, `resident.rs`: owned/bounded conversion, immutable arenas, visibility/index plans. |
| CPU geometry and indices | D3D9 `geometry.rs`, `terrain.rs`, `upload_batch.rs`, `texture_pages.rs`, `shadow_queue.rs`: lossless shadows, original encoded layouts and bounded jobs. |
| Textures / HUD | `legacy/atlas.rs`, `frame/gui_cache.rs`; D3D9 `atlas.rs`, `tiles.rs`, `animation.rs`, `screen.rs`, `gui_device.rs`, `texture_levels.rs`. |
| Enchantment glint | `legacy/glint.rs`, D3D9 `glint.rs`, entity/HUD routing in frame code. A masked additive depth-read-only pass only for enchanted geometry; supplied animation UVs and strength/tint are used. |
| Resolution scale | D3D9 `world_size.rs`, `world_target.rs`: persistent native color/depth, hardware StretchRect outside a scene, full-resolution HUD. |
| Embedded shaders | D3D9 `fog*_program.rs`, `terrain_program.rs`; sources and SDK validation in `tools/d3d9/`. No runtime shader compilation. |
| Input/boot compatibility | Win7 clipboard shim; game runtime loading/window setup and HDR stubs keep unsupported DLLs out of imports. Preserve early device choices across loading/title/world handoff. |
| Win7 COM imports | `third_party/windows-link`: only the Win7 target routes classic CoTaskMemAlloc/Free/Realloc through Ole32. Keep combase and other WinRT imports rejected; preserve the narrow patch when updating bindings. |
| Thread stack compatibility | `horizon-guest-stack/src/win7_stack.rs`: VirtualQuery walks the current reservation instead of importing Win8's GetCurrentThreadStackLimits. The upstream stack-size policy is preserved. |
| Shared game logic | Upstream `horizon-client`, `horizon-client-gameplay`, `horizon-gameplay`; do not optimize those as a substitute for improving DX9. Adapt interfaces only as needed for this port. |
| Distribution entry | Upstream moved the `horizon-client` executable into package `horizon-game`; the Win7 script must build that package with legacy-d3d9,audio. |

## Rendering and performance invariants

- Reuse Horizon's meshes, normals, tint/AO, lighting, models, CPU entity/particle
  culling and HUD. Sync/Async/Off culling runs before DX9 receives surviving vertices;
  unsupported GPU occlusion is a separate feature. Pending sections are not current
  occluders. Do not bypass conservative readiness checks.
- Bulk SectionMesh ownership moves into the existing bounded staging worker. Recycle
  buffers through the mesher's pool off the frame thread, including cancellation.
  Keep epoch/revision guards, job/byte quotas, grouping and nearest-first pressure.
- Edits remain immediately visible. Lossless CPU-shadow compression can run later on
  the same worker: 64 queued keys / 8 MiB plus one active job; pressure falls back
  synchronously. Apply completion only to the identical source Arc. Storage compression
  does not change geometry revisions, native buffers or gameplay responsiveness.
- FFP vertices are 32 bytes; HW VS2 uses exact 20/28-byte layouts when possible.
  Shadows preserve all bits, including signed zero/NaN payloads. Demand uploads and
  compaction assemble encoded bytes directly, avoiding expand/re-encode round trips.
- Native VBs/IBs respect caps and 16-bit indices. Immutable region arenas and cached
  tight index windows avoid needless updates and excessive software transforms.
  Never restore the per-frame reupload or expensive globally sorted-alpha path.
- Basic transparency is intentional. Do not implement improved transparency/OIT by
  multiplying per-frame draws, sorting or uploads on GMA. Correctness fixes must be
  evaluated for cost before being adopted. The ice fix's 23 -> 5 FPS regression was
  rejected; a simpler path recovered about 42 FPS.
- Cache sprite atlas transforms; ordinary UVs avoid polygon clipping, while repeating
  and degenerate UVs keep the general path. Geometry/textures are not reduced in quality.
- GUI uses exact bounded CPU reuse, batched streams, XYZRHW on software VP, and reused
  depth for disjoint inventory models. Keep its fast path for ordinary HUD geometry.
  Glint uses a separate masked path only when present; do not make every widget pay
  for an offscreen enchantment composite. Three-stage FFP squares sampled RGB; on
  two-stage hardware the small glint texture is pre-squared (filtering approximation).
  Keep base/foil state barriers even on one atlas page. Hardware VP reuses the
  existing TEX2 GUI upload for foil; transient foil uploads would invalidate its
  staged serial. SWVP foil uses the separate transient stream, leaving XYZRHW
  ordinary GUI valid. Include glint strength/tint in cached geometry inputs.
- Scale 100% has no extra target/resolve. Scaling the world selects its target before
  Clear, then resolves before HUD. Release DEFAULT targets on Reset and when unused.
  No CPU readback is involved. The saved option survives driver/allocation fallback.
- Mip limits use upstream sprite_mipmap_limit, including resampling levels. Native
  texture levels change only on option changes; preserve/refresh current animated
  or frozen frames. Native anisotropy is capped by the driver. RGSS remains a linear
  approximation. Do not silently rewrite unsupported settings or pretend they work.
- Keep normal logs quiet: startup identity/capability and actionable warnings only.
  Dev packages enable periodic renderer diagnostics by default; the existing
  `HORIZON_D3D9_LOG_STATS=0/1` override remains. Per-frame profiler spans remain.
  The dev SWVP entity-upload and stream-binding trials are tracked in
  [legacy-d3d9-perf.md](legacy-d3d9-perf.md), including their GMA measurements.

## Supported scope and explicit gaps

Terrain/cutouts/basic transparency, models and CPU-skinned entities, held/dropped
items, armour, enchantment shimmer, HUD/inventory/chat, particles/weather, overlays,
lightmap, tint/AO, reverse world Z and forward GUI Z are supported. Sky/End effects
are simplified. Verify glint on held items, GUI/inventory models, dropped items and
armour, including alpha masks, occlusion, scrolling, strength zero and custom tint.

Clouds/star field, glowing outlines, shader packs/core shaders, PBR, LOD/BlueMap,
GPU culling/skinning, HDR, TAA/FXAA, advanced upscaling, dynamic resolution,
frame generation, improved transparency, menu blur, custom CIT glint and split-screen
composition are not implemented. Upstream prepared GPU models fall back to CPU
geometry; new multi-view and native GPU pipeline interfaces are explicitly
unavailable. Empty mod pipeline sets remain valid. Leave unavailable controls visible and disabled
with the API reason, preserving stored values. Do not claim OpenGL/Vulkan parity.

This is currently one DX9-only EXE. A combined executable with optional modern APIs
is feasible, but requires runtime backend selection and a complete startup-import
review. Missing modern DLLs must allow DX9 fallback. Source/build-time native bundles
could carry the backend after an early seam is added; late render hooks cannot fix
an EXE that the Windows loader already refused. These remain future integration work.

## Profiling and TZ 3 baseline

The bounded P0 submission diagnostic and its hardware handoff are recorded in
[legacy-d3d9-perf.md](legacy-d3d9-perf.md). Dev packages log by default; an explicit
`HORIZON_D3D9_LOG_STATS=0` disables them for an overhead comparison.

Use game chat, without --control or a separate binary:

```
/profiler start client
/profiler stop client
```

Send the resulting local .hzperf and latest.log; do not upload automatically. DX9
has no GPU pass timestamps. Draw phases measure wall time, including driver waits;
Windows process CPU/RSS come from upstream probes. A high main-thread execution
percentage can include software VP/driver CPU work. A partly idle Task Manager
chart on the N455's two logical threads does not establish a purely GPU-bound scene.

| Capture (GMA3150) | Duration / frames | FPS (frames / seconds) | Render | Solid | Translucent | GUI | Entity draw / preparation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Nov 15 movement, before 42f0b42d47 | 92.811 s / 3310 | 35.66 | 21.38 ms | 14.68 | 0.99 | 2.84 | little visible entity work |
| Nov 18 movement, 42f0b42d47 | 64.891 s / 2254 | 34.73 | 22.06 ms | 15.55 | 0.72 | 2.73 | little visible entity work |
| Nov 18 server route, 42f0b42d47 | 49.354 s / 557 | 11.29 | 64.11 ms | 35.50 | 3.71 | 2.87 | 16.61 / 5.45 ms |

`profile-20261118-112520Z.hzperf` is movement;
`profile-20261118-113039Z.hzperf` is the server route. Their 5-second process CPU
readings are roughly 1.0–1.5 logical cores on the two-thread N455.

The moving run's upload_queue averages 0.227 ms/frame versus about 0.4 ms earlier;
retained CPU geometry is about 1.7 MiB, native arenas about 9 MiB. The log confirms
background shadow work completes and drains. Overall movement is about 2.6% slower
by frames/time, but the older route was not a fixed paired test: neither a whole-game
regression nor its absence is proved. The server route is a new reproducible baseline,
not a before/after comparison. Its entity cost is real and belongs in TZ 3, together
with terrain submission, software VP, loading spikes and GUI costs. Keep these
captures separate from a new upstream/LTO binary's results.

For every performance claim compare the same source pair, hardware, resolution,
route, distance, settings, warmup and world state. Start with scale 100%, two chunks,
Fog Off and RGB565 on GMA; compare 75/50% separately for pixel-cost sensitivity.
Benchmark loading and steady scenes separately, including p99/slow frames and memory.
A host CPU microbenchmark can validate a kernel, not establish GMA gameplay FPS.

## Build, CI and validation

`tools/build_win7_client.sh` pins nightly-2026-11-11, uses rust-src/build-std and the
Win7 GNU target, static native support and the portable CPU flags above. Install
its pinned cross tools with tools/bootstrap_win7_cross.sh. Set WIN7_ARCH=x86 for
32 bit; default x64. An early COM-only import probe runs before the full game build and is never shipped.
The optimized legacy profile uses Thin LTO, one codegen unit,
opt-level 3, panic=abort and stripped debug info. Keep linker memory bounded through
build parallelism; do not turn LTO off silently. Benchmark its effect honestly.

Actions keeps DX9 Win7 package/audit/tests, SDK shader validation and real x64/x86
Windows tests comparing the Win7 stack query with the modern API. Upstream
release/site/launcher/telemetry/mod publishing bots are not this fork's CI. Future
merges must not reactivate them accidentally. Only x86 and x64 distribution
artifacts are uploaded, as flat ZIPs containing EXE, TESTING.txt, build-info.txt,
SHA256SUMS and the architecture's strict PE import report. No inner ZIP, smoke EXE,
horizon-ctl, assets or saves. Artifact retention is finite.

Validation must cover CPU geometry/queue/index/mask contracts, Win7 architecture,
subsystem and static/delay imports, Windows DX9 compilation and SDK shader checks.
Never solve an import rejection by allowing an unavailable Win8/10 API. A successful
build does not verify rendering on GMA. Record actual hardware checks separately.
