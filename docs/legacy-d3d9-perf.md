# TZ3 engineering log: P0 submission baseline

2026-11-19. Source baseline (local checkout and fetched `dx9-dev`):
`9ef29e6bba219e1805f5be1d6636b67a0cfce0c6`. Upstream remains `df50a07e73`.
Work goes directly to the owner's `dx9-dev`, as requested. No FPS improvement is
claimed. This is **P0a**, a bounded first diagnostic patch, not completion of P0
or acceptance of TZ3. Dense SWVP windows remain the next experiment after evidence.

## Original captures re-read

The six original files were decoded with Python protobuf classes generated from
`crates/horizon-perf-format/proto/horizon/perf/v1/capture.proto` at the baseline,
after gzip decompression. Means below divide SpanSummary wall totals by its frame
count; nested stages are not added to their parent. FPS is count / Stop.t.

| Capture | Seconds / frames | FPS | Renderer ms | Solid ms | Translucent ms | Entity DRAW ms | GUI ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| GMA standing | 59.923 / 1905 | 31.79 | 25.777 | 11.872 | 9.510 | 0.025 | 2.668 |
| GMA serverwalk | 79.341 / 973 | 12.26 | 57.810 | 31.190 | 3.486 | 13.205 | 4.982 |
| RTX DX9 | 66.790 / 11132 | 166.67 | 4.383 | 1.879 | 0.605 | 0.078 | 0.079 |
| RTX OpenGL | 74.790 / 16463 | 220.12 | 3.359 | — | — | — | — |
| RTX DX12 | 69.378 / 23688 | 341.43 | 2.290 | — | — | — | — |
| RTX Vulkan | 66.805 / 24902 | 372.76 | 2.137 | — | — | — | — |

Serverwalk's 5–10 s / 35–40 s blocks have Entity DRAW means of 32.962 / 35.321 ms
per frame. The separate upstream entity preparation is outside this work's scope.
DX9 captures have empty Build.commit; modern version names `dd67b76`. Build.profile
alone does not prove matching compiler/LTO flags. These are not a paired API test.

All six captures have zero GpuPasses/GpuFrames and no timeline chunks. Exact
capture-wide frame or renderer p50/p95/p99 cannot be reconstructed. The frame
histogram bounds GMA standing p50/p95 to [30,40) ms, p99 to [50,75) ms; serverwalk
to [75,100), [100,150), [150,250) ms respectively. TimingSummary's slowest 1% mean
is 137.297 ms standing and 162.165 ms serverwalk. Do not substitute per-second
median averages for capture-wide percentiles.

`gmalog.log` confirms Chell 1.8b, GMA3150 HAL SWVP, no hardware VS, PS2, resize to
1024×600, six sprite pages and 54 animated layers. Periodic stats were absent.
Original captures/logs remain external; do not commit player data.

## Diagnostic implementation

`horizon-client-d3d9/submit-profile` compiles a fixed-size collector (compile-time
limit 16 KiB per device, no COM resources). Without this feature, COM accounting
macros expand to the original expression, and pass markers are empty inline
methods. The renderer's original coarse diagnostics remain available.

`legacy-d3d9-dev` forwards through the game/client/render manifests to
`horizon-client-d3d9/dev-diagnostics`, which includes `submit-profile` and defaults
logging on. These manifest changes only select DX9 diagnostics; gameplay code is
unchanged. `tools/build_win7_client.sh` selects it on `dx9-dev`; `WIN7_D3D9_DEV=1`
selects it explicitly elsewhere and `WIN7_D3D9_DEV=0` builds the ordinary path.
`HORIZON_D3D9_LOG_STATS=0` overrides dev logging off; `=1` enables it. No player
settings are read or written by these switches. Release profile, build-std,
ThinLTO, CPU flags, audio, package layout and import policy are retained.

| File(s) | Responsibility |
| --- | --- |
| D3D9 `submit_profile.rs` | Fixed pass counters, bounded NumVertices histogram, sampled existing thread CPU timer, window log |
| D3D9 `device.rs`, `gui_device.rs` | Account at the actual COM call sites, after existing cache decisions; successful draw arguments and copied bytes |
| D3D9 `lib.rs`, `Cargo.toml` | Diagnostic API/features and shared env/default policy |
| Render `legacy/frame.rs`, `legacy.rs` | Pass boundaries, all-frame window report, error/Present closure, shared logging policy |
| Render/client/game `Cargo.toml` | Feature forwarding only |
| `tools/build_win7_client.sh` | Dev feature selection, embedded source/compiler identity, sidecar build metadata |
| `packaging/win7-client/TESTING.txt` | Packaged default-log and same-scene overhead instructions |

The pass labels are Outside, Begin, Prepare, Solid, DynamicEntities,
StaticEntities, Translucent, Effects, Gui, Present. Prepare includes texture
patches/animations, visibility and residency work. Solid includes cold draws and
index-plan work. DynamicEntities includes world primitives and the early/late
`frame.entities` calls. StaticEntities covers the visible static section loop.
Effects includes sky, additive/through overlays, weather, lines, hand and resolve.
Gui covers the existing GUI path. Thus these new boundaries do not exactly match
every existing profiler span; they must not be added to the old span totals.

Every ~5 s, `D3D9 submit_window` gives elapsed seconds, started/presented/aborted
frames, followed by one `D3D9 submit pass=...` line per pass. These are **window
totals**, not last-frame values. Initial Outside totals include startup and pack
loading; later Outside totals include uploads before render. Failed frames are
included explicitly, not silently divided into successful frames. Reset and pack
reuse keep the collector with its device owner; no native resource is retained.

`D3D9 submit_schema` names the fixed array order. `com` counts actual COM calls
attempted (including ones returning errors); `failures` counts their errors.
It covers DIP/DP, stream/indices/texture/transform/VS/PS/render/sampler/TSS calls,
VB/IB create/lock/unlock, ordinary texture patch lock/unlock and Clear.
`cache_requests` means render/sampler/TSS requests reaching those low-level
caches, after the whole DrawState cache; `state_requests` and `transform_requests`
count facade requests. These are different levels, not interchangeable counts.

`dip_num_vertices` sums the successful DIP NumVertices arguments, including index
holes. It is **not** a unique vertex count or a measured SWVP transformation count.
`dip_ranges`, min/max, and p50/p95 power-of-two upper bounds describe these
arguments; quantiles are approximate bucket bounds. DP vertices, XYZRHW screen
vertices, triangles and lines are separate. `upload_bytes` records copied payload
bytes for immutable VB, transient VB, IB and ordinary texture patches, not driver
bus traffic or newly allocated capacity. Option-change mip replacement/readback
helpers and arbitrary driver-internal operations are outside this accounting.

No timers, strings or allocations are added per draw. All-frame pass wall timers
run at pass boundaries. Once per 64 started frames, the same boundaries also use
the existing `thread_cpu_time()`; only paired successful CPU samples contribute
to sample_cpu_ms/sample_wall_ms, with unavailable counts reported. Sum sampled
segments for a pass before forming a CPU/wall ratio. CPU duration includes driver
CPU and SWVP, and its Win7 resolution limits short-pass interpretation. It does
not identify GPU execution. `gpu_timing=not_instrumented` is explicit: query support
has not been tested and must not be described as unsupported hardware.

## Owner's first hardware run

Use the same dev EXE for both diagnostic overhead modes; keep graphics settings
and scene fixed. The dev package already enables logs, so no environment setup is
needed for the first run. Warm up the exact standing scene, then:

```
/profiler start client
# stay in the scene 60–90 s
/profiler stop client
```

Repeat for the fixed server route, preferably twice. Preserve each `.hzperf` and
its `latest.log` before the next run. Record package SHA256/build-info, actual
window/color/VP, driver, settings and route. Then restart the same EXE with
`HORIZON_D3D9_LOG_STATS=0` and repeat, preferably in A-B-B-A order. This measures
the whole diagnostic mode's overhead; compiled-out production comparison is a
separate build. Never alter resolution/settings automatically for this test.

Supply these pairs on GMA first, then an RTX DX9 control and HD4570 smoke when
available. Check the startup SHA/caps, nonzero Solid/Translucent/Gui totals,
separate static/dynamic entity totals where present, and failures/aborted counts.
Check fog/light, water/glass layering, glint/skin/items/armour, static block
entities, HUD/chat, particles/weather/animations, resource reload/pack/world
change, resize, minimize/restore and device loss. These visual/hardware checks
have not been performed in the Linux build environment.

## Remaining P0 and next decision

Local checks: 29 D3D9 unit tests (including sparse NumVertices accounting and
aborted-frame pass attribution); diagnostic Clippy with warnings denied on Linux
and Win7 x64; Win7 x64 compile without the diagnostic feature; 15 import-policy
tests; public-tree and layer checks passed. The preliminary x64 COM ABI probe
passed the strict Win7 import audit. The modified Rust module graph passes
`cargo fmt --all -- --check`. Full `pr_ready` also scans upstream standalone
gameplay files and reports inherited formatting failures, including
`game/world/persist/passenger_graph.rs` and `game/world/runtime_levels_tests.rs`;
both were checked against the baseline and are byte-identical. They are outside
this patch. No shader bytecode changed; an SDK recompile and hardware visual,
Reset/resize, x86/x64 runtime and HD4570 smoke tests have not been performed here.

The first GMA counters and on/off captures have now been supplied (see below).
P0 as a whole is not accepted: the independent RTX control and remaining
instrumentation are still open.
Per-material/page/overlay/atlas-clipping counters, entity convert-vs-submit and
GUI patch/cache attribution, solid density/reuse distributions, asynchronous
GPU queries and synthetic controlled driver tests are still open. This patch
does not infer their values from wall times. First use the split pass table and
NumVertices/draw volumes to choose the next small measurement or gated SWVP
dense-window experiment. No geometry/state optimization has been accepted yet.

## GMA on/off results, analyzed 2026-11-21

Owner supplied `standing_log.hzperf`, `walk_log.hzperf`, `latest_1.log` (logging
on) and `standing.hzperf`, `walk.hzperf`, `latest_2.log` (off), recorded on
2026-11-20. They report zero external background load and repeatable standing
and walk scenes. These are two application launches with longer recordings.
LOG1 confirms clean source `fc813ed9fece4e115380533c5c7d0827ab26c045`, pinned
compiler, GMA3150/Chell 1.8b, HAL SWVP/FFP/PS2, RGB565, INDEX16 and SYSTEMMEM
static VBs; both logs confirm the adapter/VP/formats and 1024×600 during captures.
LOG0 suppresses the new SHA line, so its same-EXE identity also relies on the
owner's pairing. No settings were changed by the diagnostic mode.

| Scene | Logging | Seconds / frames | FPS | Renderer ms | Solid ms | Entity DRAW ms | Translucent ms | GUI ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| standing | on | 120.579 / 4915 | 40.762 | 19.055 | 8.487 | ~0.02 | 7.001 | 2.063 |
| standing | off | 120.522 / 4395 | 36.466 | 22.503 | 10.343 | ~0.02 | 8.429 | 2.236 |
| walk | on | 150.952 / 1707 | 11.308 | 63.131 | 32.888 | 15.831 | 3.792 | 5.023 |
| walk | off | 153.371 / 1727 | 11.260 | 62.939 | 32.633 | 15.992 | 3.770 | 5.070 |

Walk's differences (+0.43% FPS, +0.30% renderer wall with logging) show no large
diagnostic slowdown in this pair; they do not establish exact zero overhead.
Standing's higher logged FPS is not a logging speedup. In the unlogged capture,
Solid changes from ~8.5 ms to ~12.1–12.5 ms over a long middle interval, then back
to ~8.4 ms; translucent changes in parallel (~7 → ~10 → ~7 ms). This occurs within
one recording, even with zero external background load. Frequencies/thermals/GPU
clocks were not captured, so the cause is unknown. Longer runs expose this change
but do not replace independent repetitions. No new calibration run is required
to select the first experiment.

Use complete native windows wholly inside capture boundaries, with a conservative
one-second margin for log timestamp truncation: standing 23 windows / 4716 frames;
walk 29 / 1646. All selected windows have zero aborted frames and zero recorded
COM failures. Frame-weighted native means (new pass boundaries):

| Scene / pass | Wall ms | DIP/frame | Sum NumVertices/frame | Mean DIP width | VB locks/frame | Transient bytes/frame |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| standing solid | 8.522 | 43.00 | 24624 | 573 | 0 | 0 |
| standing translucent | 7.019 | 26.00 | 20108 | 773 | 0 | 0 |
| walk solid | 32.734 | 285.59 | 86552 | 303 | 0 | 0 |
| walk dynamic entities | 13.661 | 115.96 | 8401 | 72 | 115.96 | 268823 |
| walk static entities | 2.896 | 18.03 | 1993 | 111 | 18.03 | 63763 |
| walk translucent | 3.765 | 18.86 | 10234 | 543 | 0 | 0 |

The heaviest logged dynamic window reaches 37.64 ms/frame and 362.3 DIP/VB locks
per frame. Static windows can also reach 13.65 ms; they are not the average
majority. A persistent static cache alone cannot remove the dynamic cost.
Existing arenas/plan reuse and GUI preprojection are active; this is confirmation
of existing features. Terrain performs no per-frame VB uploads in these windows.

Terrain standing solid/translucent both take ~0.35 microseconds per requested
vertex despite different texture-bind counts (2 vs 23/frame). Walk gives
~0.38/~0.37. These descriptive ratios and the large vertex volume make blindly
widening SWVP index windows risky; they do not prove pure transform cost or rule
out GPU pixel work. Sparse per-pass CPU samples are too coarse for exact stage
percentages (some CPU/wall ratios exceed 1). GPU timestamps remain unimplemented.

Next bounded experiment: P1-B1 upload granularity for dynamic entities, before
the separate persistent-static-cache change. Preserve existing material/alpha
order, draw count and tight NumVertices; reduce ordinary base-geometry VB locks.
Keep logging mode equal between baseline/candidate. No FPS gain is accepted yet.

## Dynamic entity staging experiment (dev default, hardware result pending)

The first candidate changes only upload granularity for `draw_entities` phases
0/1 on software VP. Existing per-quad conversion, HUD-page batches followed by
sprite batches, state/texture draws and final glint order stay intact. It copies
already converted base vertices into one scratch vector, calls the existing
DISCARD/NOOVERWRITE ring once, then draws each original batch using a checked
base offset and tight `MinIndex=0` / `NumVertices=batch length`. The shared INDEX16
quad IB and full 32-byte FFP vertices are unchanged. It neither expands quads to
six vertices nor merges material draws. Glint, static block entities (phase 2),
hand, effects and hardware VS2 retain the previous submission path.

Only dev SWVP starts with staging enabled (`B_staged`). The owner requested dev
defaults instead of additional experiment environment variables. Focused
`Ctrl+Shift+F12` switches between `B_staged` and `A_baseline` at frame boundaries,
without restart or saving settings. The mode is logged; an existing native
counter window is emitted before switching, and the next five-second window
starts afresh. Chat captures contain corresponding `d3d9_entity_mode_*` parent
spans and `d3d9_dynamic_entity_convert` / `d3d9_dynamic_entity_submit` children.
The latter submit span includes consolidation, buffer upload, draws and glint;
it is elapsed wall time, not pure CPU or GPU time. Dev/production selection uses
the existing `legacy-d3d9-dev` feature; normal packages do not select this path.

The scratch reservation is bounded by `Device::max_vertices()` (at most 65532
vertices / 2097024 payload bytes), accounted in the renderer CPU census. There
is one extra CPU copy and no additional native VB, immutable shadow or geometry
cache. A whole phase exceeding the limit or a failed reservation uses baseline
unchanged. A failed stage before any draw falls back; device-lost errors propagate
to the existing lifecycle. Staged tokens hold only the owner/generation/range,
no DEFAULT COM resource. Another upload or Reset invalidates them; they are
consumed before glint uploads. No cross-frame geometry survives to require pose,
lighting, resource-pack or section-revision invalidation.

Expected result to test, not an accepted gain: fewer DynamicEntities VB locks at
the same route locations, with unchanged draw count, sum NumVertices, triangles,
payload bytes and image. Base geometry needs at most one lock per eligible
early/late call; glint/world-primitive uploads and whole-phase fallbacks remain,
so total DynamicEntities locks need not equal two. Texture binds and SWVP work
are deliberately preserved. Extra copying, larger DISCARD reservations or driver
behavior can make this slower. Static entities and solid terrain are separate
experiments and have not been optimized by this patch.

Use the same new dev EXE for both live modes with logs on. Record the warmed-up
fixed route separately in B/A/A/B where practical, returning to its same start;
do not repeat the old log-on/off calibration. Check the nearly entity-free
standing scene for regression, plus skins, glint/armour/items, alpha/depth,
resource/world changes, resize and minimize/restore. Preserve the `.hzperf`
captures and the shared log. GMA, HD4570, visual/Reset and hardware pacing
acceptance remains pending. Work is committed directly to `dx9-dev` at the
owner's request; no integration PR to `main` is part of this iteration.

Changed implementation files: D3D9 `device.rs` / `geometry.rs` add checked staged
quad submission; `dev_control.rs`, `lib.rs` and its Cargo feature list provide
the focused dev hotkey and dev-build selection; legacy `frame.rs` / `legacy.rs` add
the bounded stream, fallback, mode labels and wall spans; `legacy/compat.rs`
accounts for its CPU storage. Packaging `TESTING.txt` describes the live pair.
No shaders, game inputs, settings files or upstream preparation change. The
existing Win7 package workflow also runs on pushes to `dx9-dev`, so direct dev
commits produce both packages without a PR. Its build/audit steps are unchanged.

Local candidate checks: 31 D3D9 CPU unit tests passed (including ring-offset
range validation and one-toggle-per-key-press behavior); D3D9 Clippy with warnings
denied passed on Linux and Win7 x64 with diagnostics, as did a Win7 x64
diagnostics-off compile. The Windows dev renderer compiles. Workspace module
formatting, 15 import-policy tests, layer and public-tree checks passed. These
checks establish neither a driver speedup nor the visual/Reset cases above.
An unfiltered renderer/dependency Clippy run stops on the inherited
`unnecessary_lazy_evaluations` in
`horizon-world/src/worldgen/structure/piece_limited.rs:534`; that file is
byte-identical to the tested baseline and is outside this renderer patch.
