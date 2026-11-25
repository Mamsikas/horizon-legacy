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

## Dynamic entity staging experiment (a974 history; subsequently reverted)

This section records the a974 implementation. The hardware outcome and rollback
below supersede its live-switch/default instructions.

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

## Package validation for a974320ab6

Binary source: `a974320ab69ba16959fd1b96ac31589a8c739406`; measured input
baseline: `fc813ed9fece4e115380533c5c7d0827ab26c045`. Both jobs of
[Win7 run 44867167769](https://github.com/Mamsikas/horizon-legacy/actions/runs/44867167769)
succeeded on 2026-11-21. Each compiled the full client with pinned build-std,
legacy ThinLTO and the existing SSE targets, passed the Win7 client import audit
and COM ABI probe, 29 normal D3D9 CPU tests and 15 import-policy tests.

| Package | Artifact | GitHub uploaded-ZIP SHA256 |
| --- | --- | --- |
| x64 | [14692957442](https://github.com/Mamsikas/horizon-legacy/actions/runs/44867167769/artifacts/14692957442) | `d0f449a0b62c9e73b4e970cdad9d9c66008ff93611ba0c4385c5797d5033d98c` |
| x86 | [14692981730](https://github.com/Mamsikas/horizon-legacy/actions/runs/44867167769/artifacts/14692981730) | `6e34a49f41b889ef863d4d078f2b9fa607764d387eb17522aea5667dab6403ee` |

API digests match the CI upload logs. Independent local ZIP rehash/extraction
was blocked by HTTP 403 from the environment's file-storage proxy; these are
GitHub's digests. Retain package `build-info.txt` and internal `SHA256SUMS`.

The full local `pr_ready --quick --base fc813ed9fece4e115380533c5c7d0827ab26c045`
completed on the unchanged candidate worktree in 38m28s and is **not green**.
It reports nine standalone-format command failures in the two unchanged gameplay
files noted above, workspace Clippy warnings, and Python 1985 run / 4 failures /
63 errors / 13 skips. Displayed Python failures include removed upstream runner
and coverage assumptions and a prebuilt-game XZ fixture mismatch; remaining
Python causes were not diagnosed in this DX9 iteration. Rust tests report 2690
passed / 32 failed; the failures are in modern/wgpu renderer tests. Their causes
were not diagnosed here either. Ten doc-tests passed. Public-tree, repository
hygiene, layer, address, export-list and test-stand-in checks passed. No unrelated
upstream test or gameplay code was changed to make this run green.

Next evidence is the complete previous ~150-second GMA walk, using the same new
EXE in B/A/A/B where practical, with separate chat captures and one shared log.
Hold the focused hotkey about half a second at low FPS; keep diagnostics enabled
and remove the previous `HORIZON_D3D9_LOG_STATS=0` override. Standing is the
regression control. Hardware FPS/image/Reset/HD4570 acceptance remains pending;
no optimization win is claimed by these builds.

## a974 GMA result and rollback, 2026-11-21

The owner supplied `profile-20261121-061823Z.hzperf` (standing),
`profile-20261121-062238Z.hzperf` (walk) and `latest (12).log`. Startup confirms
clean a974320ab69ba16959fd1b96ac31589a8c739406, the pinned compiler, GMA3150
Chell 1.8b, HAL SWVP/FFP/PS2 and 1024x600/RGB565. Both captures contain only
`B_staged`. The first logged hotkey transition was 13:25:37, after the walk
ended at about 13:25:08; the owner correctly reported recording B only. The
switch lacked visible feedback and is removed rather than requiring the owner
to infer its state from logs.

The existing fc813ed captures are valid reference A runs. Restarting the game
between matched builds does not invalidate an A/B. A same-EXE switch was an
optional control, not an acceptance requirement. Small differences still need
repetition and comparable workload before assigning a causal speedup.

| Capture | FPS | Renderer ms | Solid ms | Entities ms | Translucent ms | GUI ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| fc813 standing, log on | 40.762 | 19.055 | 8.487 | ~0.02 | 7.001 | 2.063 |
| a974 standing B | 35.340 | 23.173 | 10.699 | 0.029 | 8.439 | 2.401 |
| fc813 walk, log on | 11.308 | 63.131 | 32.888 | 15.831 | 3.792 | 5.023 |
| a974 walk B | 11.529 | 61.787 | 32.734 | 14.926 | 3.701 | 5.090 |

In wholly capture-contained native windows, walk DynamicEntities VB locks fell
115.96 -> 4.66/frame (-96.0%), while DIP stayed close (115.96 -> 114.50), summed
NumVertices 8401 -> 8442 and transient payload 268823 -> 270143 bytes/frame.
All selected a974 windows were B, with zero stage fallbacks, aborted frames or
recorded COM failures. The mechanism worked on real GMA. Dynamic pass wall barely
changed (13.661 -> 13.433 ms); overall walk FPS differs by only +1.95%, which is
not an established gain. Static pass wall/draw count also changed despite its
code being untouched (2.896 -> 2.137 ms; 18.03 -> 4.50 DIP/frame); that difference
cannot be credited to dynamic staging. Standing performed no entity staging
groups, yet its terrain/GUI were slower. Earlier unlogged standing was also
slower than logged standing (36.466 FPS), illustrating the unresolved variation.
The poorer standing result is recorded; clocks/GPU execution are not measured.

New walk spans split dynamic work into 4.888 ms conversion and 8.087 ms submit
per frame (sum the early/late sibling spans only). In the five-second block at
t=115 s, conversion reaches 14.169 ms, submit 27.646 ms and the enclosing early
entities stage 50.824 ms/frame. Do not add that parent to its children. F3's large
entity cost therefore remains real. Submission includes driver/SWVP/GPU waits,
state, draws and glint; conversion is DX9 work, separate from upstream prepare.

Decision: no accepted FPS gain justifies the additional copy/reserved 2097024
bytes. Restore the original per-batch uploads and remove the hotkey, staged-quad
API and scratch census entry; retain the convert/submit spans. This is an
unproven/neutral experiment, not a confirmed regression or improvement. No new
calibration capture or mandatory in-game switch is requested. GPU timestamps,
per-material/clip attribution and hardware compatibility smoke remain open.

## Next candidate: within-frame stream-0 binding reuse

Following rollback e21b18494f984bc51319dd40a27c7c6f0741319e, the next independent
change targets measured actual `SetStreamSource` calls: 286.8/frame in walk solid,
114.5 in dynamic entities, 43/26 in standing solid/translucent. These are setter
calls per draw, not proof that all of those calls change the bound buffer. The
copy experiment's weak wall-time result argues against retaining extra copies;
this trial instead removes redundant API requests without repacking any geometry.

Only dev software VP enables reuse. One stream-0 key stores the native VB
identity, byte offset and stride. An identical successful binding skips the
setter; a changed key submits the original call. Failed calls clear the key.
All six draw binding paths, including Full32, hardware compact calls and GUI
XYZRHW28, use the same helper. Hardware VP and ordinary packages still submit
every setter; their vertex formats and draw arguments stay unchanged.

The key is non-owning (24 bytes on x64, 16 on x86), with no allocations or extra
VB references. D3D9 already retains the currently bound VB, preventing identity
reuse while it remains bound. StateCache's existing per-frame reset clears it;
real Reset clears it before dropping/unbinding DEFAULT resources, including a
failed reset. Both dynamic rings also clear it on DISCARD as a conservative
rename barrier. NOOVERWRITE writes do not change stream identity/layout. The
only direct setter outside the helper is Reset's null bind, explicitly covered
by invalidation. Input declarations, FVF changes, render targets and transforms
are still set by their original paths. No cross-frame coherence is assumed.

`stream_requests` is an added scalar in the existing per-pass log; COM array
order/meaning and the three old cache-request counters are unchanged. It counts
logical draw binds; the COM SetStreamSource column includes actual calls,
including Reset's explicit null bind. No per-draw timer/string/allocation is
added. The hotkey and all a974 consolidation storage remain removed. There is
one default strategy per binary, identified by build SHA and the startup
`stream0_cache enabled=...` line, with no experiment environment variable.

Compare this candidate primarily with the existing fc813 reference: it restores
that upload strategy and adds only stream-bind reuse plus diagnostics. The a974
run remains useful context, but differs in upload granularity. Keep the same
settings/full route and retain logs/profiles. Expected evidence is fewer actual
SetStreamSource calls than stream_requests at matching geometry/draw volumes;
any frame-time gain, driver quirks, visual correctness and hardware pacing remain
unconfirmed. This is another bounded hypothesis, not a promised FPS improvement.

Local candidate checks passed: 30 diagnostic D3D9 CPU tests (including native
binding coherence across buffer/layout changes, failed calls, invalidation and
disabled mode), Clippy with warnings denied on Linux and Win7 x64, diagnostics-off
Win7 x64 compile, module formatting, repository hygiene and 15 import-policy
tests. Native package audits and actual GMA/HD4570 smoke are separate checks.

## Package validation for 27cfa36de1

Executable source: `27cfa36de122bcee9b97a2cb7e5b168838466fbd`, following the
separate rollback `e21b18494f984bc51319dd40a27c7c6f0741319e`. Both jobs of
[Win7 run 44878880554](https://github.com/Mamsikas/horizon-legacy/actions/runs/44878880554)
passed full build-std/ThinLTO client compilation, Win7 client import audit, COM ABI
probe, 28 normal D3D9 CPU tests and 15 import-policy tests.

| Package | Artifact | GitHub uploaded-ZIP SHA256 |
| --- | --- | --- |
| x64 | [14697666951](https://github.com/Mamsikas/horizon-legacy/actions/runs/44878880554/artifacts/14697666951) | `d9bda76fa5cd6b5e91f3df3f0a871ebc94801d08c130ab1e5797171e3088b67c` |
| x86 | [14698311086](https://github.com/Mamsikas/horizon-legacy/actions/runs/44878880554/artifacts/14698311086) | `92af47b427243fc091b4e7f72cdf21b2f9b9d1c70d850f3ea6fa31ea9bd20ac3` |

API digests match the CI upload logs; no independent local ZIP rehash was
performed because file-storage access returned HTTP 403 in the previous
iteration. Retain the package's build-info and internal SHA256SUMS.

Full local `pr_ready --quick --base e21b18494f984bc51319dd40a27c7c6f0741319e`
finished on the unchanged candidate worktree in 3m18s: 28 D3D9 tests, public tree,
repository hygiene, layers, addresses, export lists and test stand-ins passed.
The broad gate is **not green**: the same inherited standalone-format/Clippy
issues and Python 1985 run / 4 failures / 63 errors / 13 skips remain. No upstream
code/tests were changed to hide them. No prepared-data tests or GPU hardware
smoke were run here. Actual GMA time/image/Reset and HD4570 acceptance is pending.

The owner's ordinary standing and full fixed walk, with default dev logging and
the same settings, are sufficient for the next diagnostic comparison. No hotkey,
new runtime variable, extra A recording solely for a toggle, or renewed logging
calibration is required. If a useful time reduction appears, repeat it before
acceptance. If stream setters fall without a wall-time gain, investigate the
remaining conversion and draw/SWVP/pixel costs with a controlled standalone
benchmark rather than treating fewer calls as proof of higher FPS.

## Stream binding result and combined candidate, 2026-11-21

The owner supplied `profile-20261121-080627Z.hzperf` (standing),
`profile-20261121-080941Z.hzperf` (walk) and `latest (13).log`. The startup log
confirms clean `27cfa36de122bcee9b97a2cb7e5b168838466fbd`, stream cache enabled,
the same GMA3150 Chell 1.8b HAL SWVP and 1024×600 RGB565. Complete native
windows inside the recordings cover 5354 standing and 1700 walk frames, with
zero aborted frames and zero recorded COM failures.

| Recorded build | Standing FPS | Walk FPS | Walk DX9 ms/frame | Walk dynamic native ms/frame | Walk slowest 1% mean ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fc813ed9fe` reference, log on | 40.762 | 11.308 | 63.131 | 13.661 | 221.711 |
| `a974320ab6` entity staging | 35.340 | 11.529 | 61.787 | 13.433 | 218.829 |
| `27cfa36de1` stream binding reuse | 37.877 | 11.249 | 63.193 | 14.366 | 211.645 |

`27cfa36de1` sent 4 of 43 standing solid stream requests, 1 of 26 standing
translucent requests, 44.78 of 286.35 walk solid requests and 1.25 of 143.31
walk dynamic requests per frame. The measured reduction in actual calls is
84% for walk solid and 99% for walk dynamic. Standing solid/translucent retain
24624/20108 requested vertices and 43/26 draws across all three builds. Walk
solid stays near 86.6–87.0k requested vertices and 32.7–33.0 ms in native
windows. Walk dynamic draws and texture binds are higher in the latest run
(143 against 114 in the staging run), despite similar vertex volumes. The
latest dynamic submit span is 9.19 ms/frame against 8.09 in the staging run,
and conversion is 4.82 against 4.89. These elapsed measurements include waits.
Differences in work between runs prevent attributing the time changes to the
two setters and upload strategies alone. The walk percentile buckets agree,
while its slowest 1% mean is 3.3% lower than the staging run.

The staging rollback made the stream comparison independent. Its +1.95% walk
FPS result remains a small measured improvement, with a 96% reduction in VB
locks and no regression attributed to staging. Standing FPS varied across
captures that performed zero staged entity groups. Each individual trial has
one logged capture per scene, so a repeatable frame time gain is still open.

The owner approved measuring individual changes before combining them. The
next dev SWVP build restores the tested entity staging path on top of stream
binding reuse. It keeps the same 65532-vertex maximum reservation, phase and
material order, tight `NumVertices`, fallback and device loss handling as the
staging run. Its staged draws use the same binding cache as other draws. They
consume their frame-local token before glint or another upload. Both policies
are chosen at startup through the existing dev feature and SWVP caps. The log
records both policies, staged groups, fallbacks and binding calls. The earlier
hotkey is omitted. Compact hardware VS2, asset inputs and presentation settings
follow the existing paths.

Use the existing three recordings as references for a combined walk and
standing run with default dev diagnostics. Walk VB locks and actual stream
binds should fall together while requested vertices and draws track scene
workload. Compare renderer and pass wall time and the slowest 1% frames before
making a keep or revert decision. GMA image and device lifecycle checks and
HD4570 smoke for the combined build are pending.

The combined candidate passed 31 diagnostic D3D9 CPU tests, including the
restored indexed range test and the stream binding coherence test. D3D9 Clippy
passed with warnings denied on Linux and Win7 x64. The Win7 x64 dev renderer
compiled with its existing `NativePack: Send` warning. Module formatting, 15
import policy tests, layers, public tree and repository hygiene also passed.

## Package validation for a2766b3517

Executable source `a2766b3517bebd7147e11623cdefbec4ee0a46de` combines the two
individually measured changes. Both jobs of
[Win7 run 44884858572](https://github.com/Mamsikas/horizon-legacy/actions/runs/44884858572)
passed pinned build-std and ThinLTO compilation, client and COM ABI import audits,
29 ordinary D3D9 CPU tests and 15 import policy tests per architecture.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14700117916](https://github.com/Mamsikas/horizon-legacy/actions/runs/44884858572/artifacts/14700117916) | `b1075f5609289924f8469fa82e305c9268e6c671b5686596e49f073083bb3643` |
| x86 | [14700500331](https://github.com/Mamsikas/horizon-legacy/actions/runs/44884858572/artifacts/14700500331) | `8b39b5b1ec1905ec05cd54ef5a5a80fb27639f2e3cccf9ddf0e78b742aa03dbe` |

The API digests match the CI upload logs. The package carries internal
`SHA256SUMS` and `build-info.txt` for the executable and its provenance.

The ordinary Win7 x64 renderer also compiled locally with the existing
`NativePack: Send` warning. The full `pr_ready --quick` check remains red
with the previously recorded workspace fmt, Clippy and Python issues and
32 modern/wgpu renderer failures. It reports 2690 Rust tests passed and ten
doc tests passed. The focused DX9 tests and the Win7 package workflow passed.

Use this combined dev build for the next standing and walk recordings. Both
paths are selected automatically on software VP. The existing reference
and individual captures supply comparison data. A GMA timing, image and
lifecycle result for the combined build and an HD4570 smoke remain pending.

## Combined GMA measurements and the standing slowdown, 2026-11-21

The owner supplied `profile-20261121-090522Z.hzperf` (standing),
`profile-20261121-090813Z.hzperf` (walk) and `latest (14).log`. The log identifies
clean `a2766b3517bebd7147e11623cdefbec4ee0a46de`, GMA3150 Chell 1.8b HAL SWVP,
1024×600 RGB565 and both dev policies enabled. Complete diagnostic windows cover
4,202 standing frames and 1,698 walk frames without an abort or recorded COM
failure. These new captures have the client side only.

| Scene | Frames / seconds | FPS | Frame p50 / p95 / p99 buckets (ms) | Slowest 1% mean (ms) | DX9 wall (ms/frame) |
| --- | ---: | ---: | --- | ---: | ---: |
| Standing | 4,454 / 121.416 | 36.684 | [25,30) / [30,40) / [40,50) | 67.294 | 22.258 |
| Walk | 1,775 / 152.048 | 11.674 | [75,100) / [100,150) / [150,250) | 179.664 | 60.824 |

Walk dynamic draws average 116.82 with 8,461 requested vertices per frame.
Actual VB locks and stream binds fall to 4.32 and 1.13 per frame. Both reductions
appear together in the combined build. Native dynamic entity time is 13.437 ms
per frame, close to the staging-only result of 13.433 ms. Compared with the
logged reference, FPS is 3.2% higher and the slowest 1% mean is 19.0% lower in
this recording. The 115–120 s route interval still averages 45.11 ms for the
renderer `entities` span (54.56 ms on the logged reference). Its dynamic world
subspans include 12.49 ms of conversion and 23.93 ms of submit wall time. These
samples support keeping both experiments together on `dx9-dev` for now.
Repeatability, image/lifecycle validation and HD4570 smoke remain open.

### Standing

| Metric | 30–85 s | 90–110 s |
| --- | ---: | ---: |
| FPS | 31.95 | 43.40 |
| Renderer wall (ms/frame) | 25.66 | 18.73 |
| Solid / translucent wall (ms/frame) | 11.73 / 9.35 | 8.35 / 6.82 |
| Present wall (ms/frame) | 0.69 | 0.51 |
| Process CPU (logical cores) | 1.89 | 1.26 |
| System CPU (logical cores) | 1.99 | 1.36 |
| Render thread CPU (% of one logical CPU) | 80.8 | 97.8 |

The two terrain passes send 43 and 26 draws with 24,624 and 20,108 summed
`NumVertices` per frame throughout all 23 complete windows. Their per-frame
COM counter signatures agree. The solid plan is reused, there are no terrain
buffer uploads, and entity staging processes zero groups. The slowdown adds
about 6.9 ms of renderer wall time per frame while render-thread CPU per unit
wall time falls. Independent CPU windows put the increase on other Horizon
process threads at roughly 0.8 of a logical CPU. That is an estimate on the
N455's one physical core with SMT. DX9 spans include driver and scheduling time.

A similar minute-long saturation interval occurred on the `fc813ed9fe`
reference with `HORIZON_D3D9_LOG_STATS=0`. The baseline dev log also has a 29 FPS
frozen standing window before the recording starts. Across the reference and
three candidates, the sustained episode starts roughly 44–49 seconds after the
freeze message in recordings that contain it. These observations rule out a
requirement for the new submission policies, DX9 logging, or active capture
for the slowdowns. The identity of the busy thread and the trigger are open.
DX9 runtime or driver work on another thread is still possible.

The client capture also shows separate 49–58 ms `control` spikes near its
10-second memory snapshots. `poll_profile` collects the memory report and can
wait for the integrated server. This upstream capture overhead accounts for
individual spikes. For the sustained slowdown, existing Windows captures have
no per-thread CPU samples, server spans or GPU timestamps. A single standing
trace with both `/profiler` sides and external process/thread CPU deltas can
correlate the additional work with the server and renderer on the same EXE.
No rendering or game behavior is changed for this diagnosis.

## Static block-entity buffer experiment

On the combined-build route, complete log windows near 115–125 s put dynamic
entities at 29–35 ms/frame and static block entities at 8–9.3 ms/frame. The
static pass repeatedly uploads up to 302 KiB of converted vertices per frame.
Dynamic draws are still small and texture-heavy, with 341 draws and 340 actual
texture binds per frame in one heavy window. Static reuse is a separate test
that can save conversion and uploads without regrouping those blended draws.

The dev SWVP path retains one immutable system-memory VB for a supported
visible section. It runs the same entity conversion and atlas clipping on a
cache miss, and keeps the original ordered page/state ranges. On a hit it
uses the current transform, lightmap, filtering and fog. The first trial covers
plain block-entity quads (low overlay bits zero). Sections with other overlays
take the existing transient path. The same INDEX16 quad buffer is reused and
each draw's `NumVertices` stays as tight as before. Static cache hits can incur
a stream bind per section where the transient path shares a VB across sections.

The existing published catalog revision gates pruning of unloaded or changed
sections. Each entry also checks its section revision. Cache storage belongs to
the renderer and is cleared on world clear, device Reset, atlas tile-presence
changes and Nether lighting mode changes. Pack reload constructs a new
renderer. Current atlas pixels and lightmap updates still reach existing
ranges. Allocation failure or exhausted budget falls back to existing draws.
The cache holds at most 256 entries, with vertex and range payload bounded by
the lesser of 8 MiB and one eighth of the existing mesh budget. Cache uploads
are limited to four section builds and 512 KiB per frame. Builds reuse the
existing entity staging scratch. Hits keep no CPU vertex copy besides D3D9's
system-memory VB. The hardware VS2 path continues to use the previous renderer.

Compare the next route with `a2766b3517`. Use `StaticEntities` draw, lock, bind
and byte counters, `d3d9_static_entity_build` and `d3d9_static_entity_submit`
spans, and the hit/miss/fallback totals in `D3D9 static entity cache`. Track
first encounters and stationary reuse, plus changed models, block lighting,
unload/reload, resource pack and resize/Reset. Capture standing on the same
build with both profiler sides and the separate thread CPU utility. The standing
probe can be analyzed independently of the static experiment because the
reference standing scene has no material static-entity draws.

## Package validation for 1c7f544842

Source `1c7f54484286a4b3e907191a24928c8b39aeb56e` was checked out by both jobs of
[Win7 run 44892116375](https://github.com/Mamsikas/horizon-legacy/actions/runs/44892116375).
Pinned build-std/ThinLTO packaging, the client and COM probe import audits,
the 15 import-policy tests and 29 CPU DX9 tests passed for both targets.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14702678479](https://github.com/Mamsikas/horizon-legacy/actions/runs/44892116375/artifacts/14702678479) | `4b40effe28db72671b05666dea9582bf99d6020322a3f32f44e36c7e00759d57` |
| x86 | [14702859381](https://github.com/Mamsikas/horizon-legacy/actions/runs/44892116375/artifacts/14702859381) | `2bcfdb3b37b8d95bff81d9cf09705dd512b572b8769b6f491cb26f8cd1413386` |

GitHub's artifact digests agree with the upload job logs. The packages include
`build-info.txt` and `SHA256SUMS` for the tested executable. The renderer
also passed local optimized Win7 x64/x86 compile checks with and without dev
diagnostics and a Win7 x64 test compile. The local DX9 diagnostic suite passed
31 CPU tests. The full `pr_ready --quick` remains red on the previous Python,
format and Clippy checks and the same 32 modern wgpu tests. Runtime cache
reuse, visuals and Reset still need the GMA test. HD4570 hardware smoke is open.

For interpreting the existing combined reference, both logged routes contain
a common 0–150 s interval. The combined run draws 54 more frames there, 34 of
them in the first 20 s. From 20–150 s the reference and combined averages
are 11.61 and 11.76 FPS. The 100–125 s interval is 8.48 and 8.96 FPS, with
entity draw wall time of 36.12 and 33.39 ms/frame. Early long stalls contribute
to the difference in the capture-wide slowest 1% mean. Use the stage and
upload counters alongside route FPS to evaluate this separate experiment.

## GMA static-cache result and entity pipeline, 2026-11-21

The owner recorded `profile-20261121-111352Z.hzperf` (walk),
`profile-20261121-111105Z.hzperf` (standing with the integrated server),
`latest (15).log` and a standing `GetThreadTimes` CSV. The log identifies clean
`1c7f54484286a4b3e907191a24928c8b39aeb56e` on GMA3150 HAL SWVP. Use
`a2766b3517bebd7147e11623cdefbec4ee0a46de` as the static-cache reference.

| Walk metric | Reference | Static cache |
| --- | ---: | ---: |
| FPS, whole recording | 11.674 | 11.605 |
| DX9 wall, ms/frame | 60.824 | 61.213 |
| Slowest 1% mean, ms | 179.664 | 235.894 |
| Static pass wall, ms/frame | 2.173 | 1.071 |
| Static pass index draws/frame | 4.52 | 4.60 |
| Static pass vertex locks/frame | 4.52 | 0.11 |
| Static pass vertex uploads, KiB/frame | 63.27 | 1.56 |
| Dynamic pass wall, ms/frame | 13.437 | 14.338 |
| Dynamic pass index draws/frame | 116.82 | 144.56 |

The native comparisons use complete log windows (1,698 and 1,646 frames).
The new walk has 1,759 frames in 151.568 s. The p50/p95/p99 frame buckets are
still [75,100), [100,150) and [150,250) ms. Cache reuse cuts the measured static
pass roughly in half at comparable vertex/draw counts. The reported retained
payload peaks at 378,496 bytes. Whole-route FPS and frame tail do not establish
a performance gain. The 0–20 s block averages 8.90 versus 10.95 FPS, with
several of the new worst frames near 10–11 s including long `tick` and network
work. The 20–150 s block averages 12.01 versus 11.76 FPS. The 100–125 s block
averages 8.84 versus 8.96, and 115–120 s `entities` is 55.01 vs 45.11 ms/frame.
These are route-time matches, with different work between runs.

The heaviest native dynamic window ends near 18:15:54 local time. It has
53.93 ms/frame, 678.10 indexed draws and 678.59 `SetTexture` calls per frame.
The stream cache sends one stream bind per frame there. Render, sampler and
texture-stage cache probes total about 26.7k/frame, though actual setters for
these three groups total roughly 29/frame. Earlier staging and the static cache
target different work. The dynamic structure now worth measuring is ordered
material runs and texture page locality.

In pinned upstream `df50a07e73accd45b6af17905ede4d5478e9fc48`, `EntityBatch`
reuses its CPU allocations, packs ordered runs into a common vertex buffer,
and draws the ordinary material with the same bound 2D entity atlas and
lightmap. A scene without prepared models can draw that material in one indexed
call. Prepared models and special passes have their own ranges. Static block
entities reuse revisioned per-section buffers. The vertex shader applies face
lighting, and the GPU poses supported models when the relevant path is enabled.
GMA has a 2048×2048 texture limit and uses FFP software VP, so the DX9 version
needs a physical page mapping and preserves its current alpha and phase order.
Measure the HUD- and sprite-page working sets and adjacent same-state intervals.
A compact 2D atlas for frequently used dynamic tiles could merge such intervals
without permuting vertices. Tile padding, pixels, sampling, texture updates and
fallback are part of that experiment. The data do not yet give a draw-count
forecast for any layout.

### Standing CPU attribution

| Recording interval | FPS | Process CPU (logical cores) | Window thread | TID 2436 | Thread started in d3d9.dll |
| --- | ---: | ---: | ---: | ---: | ---: |
| 15–25 s | 42.10 | 1.251 | 0.952 | 0 | 0.251 |
| 30–80 s | 29.44 | 1.902 | 0.750 | 0.837 | 0.167 |
| 95–105 s | 42.10 | 1.248 | 0.959 | 0 | 0.243 |

Only complete CPU sample intervals enter this table. The measured thread sum
accounts for 1.899 of the 1.902 process CPU cores on the slow plateau. The probe
uses roughly 0.008 logical cores there. TID 2436 starts in `horizon-client.exe`
at the same generic entry RVA as multiple other Rust threads, so the active
function remains unknown. The integrated server stays at 20 TPS and its tick
spans show no comparable sustained increase. Terrain and entity submissions
stay stable, DX9 staging is idle, and its compaction total is unchanged. Prior
builds showed the plateau with DX9 logging disabled. The new trace localizes
the contention to another client-process thread without proving its call stack.

### Block-entity visibility and cache invalidation

Terrain uses `in_view_distance` around the camera's column with a one-column
margin per axis. Resident columns and block-entity visibility have separate
life cycles. Both renderers get the same live block-state filter and each
renderer keeps the coarse frustum and 64/256 block reach checks for block
entities. A model beyond two nominal chunks can therefore still come from a
loaded column. The captures do not contain the coordinates and draw identity
for the reported object. They cannot distinguish that case from a stale model.

DX9's column removal now advances the catalog revision when it removes a list
of block entities, so the shared catalog and DX9 cache can release obsolete
snapshots. The DX9 adapter also honors the existing piston-position invalidation
hook. It removes the named entry and advances its section's block-entity
revision while the terrain mesh revision stays valid for its worker and arena.
That closes a demonstrable stale-snapshot path when another block entity in the
section keeps the section visible before the remesh. The visibility radius and
terrain geometry follow the existing rules. Win7 x64 with test compilation and
the Win7 x86 optimized renderer check passed. Movement and unload smoke still
require hardware.

## Win7 packages for 2129b7999a

Both jobs of [Win7 run 44899838360](https://github.com/Mamsikas/horizon-legacy/actions/runs/44899838360)
packaged source `2129b7999aa35922cacbcc2cd6c395e5197499c4` with the existing
build-std/ThinLTO profile. The import audit, 15 import-policy tests and 29 DX9
CPU tests passed on both architecture jobs. GitHub ZIP digests match the job
logs. The block-entity regression test was compiled in the local Win7 x64
renderer test check. The movement and unload behavior still needs a GMA smoke.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14706330886](https://github.com/Mamsikas/horizon-legacy/actions/runs/44899838360/artifacts/14706330886) | `f320a4285ddf4d9b8f8b49d2ad658768b386b3b5508201912b761a4f9ec87bc4` |
| x86 | [14705399052](https://github.com/Mamsikas/horizon-legacy/actions/runs/44899838360/artifacts/14705399052) | `6684d42f503043b0337f53594b115e3ada1ef10cbbfdb0e2a96c4375ef6bf72d` |

## Dynamic entity atlas, 2026-11-21

The updated GMA recordings identify clean
`2129b7999aa35922cacbcc2cd6c395e5197499c4` through the native log.
`profile-20261121-121057Z` recorded 1,825 walk frames in 152.900 s.
`profile-20261121-120822Z` recorded 4,409 standing frames in 121.106 s.
The profiles contain client spans and frame histograms. GPU timestamps and
sampled CPU stacks are absent. The native log's complete windows cover
1,742 walk and 4,179 standing frames with zero aborted frames or tracked
COM failures.

| GMA walk metric | `1c7f544842` | `2129b7999a` |
| --- | ---: | ---: |
| FPS, full capture | 11.605 | 11.936 |
| Renderer wall, ms/frame | 61.213 | 59.258 |
| Slowest 1% mean, ms | 235.894 | 219.586 |
| Solid terrain wall, ms/frame | 32.448 | 32.393 |
| Dynamic entity pass wall, ms/frame | 14.338 | 12.507 |
| Dynamic indexed draws/frame | 144.56 | 94.95 |
| Dynamic SetTexture calls/frame | 144.20 | 94.58 |
| Dynamic indexed NumVertices sum/frame | 8,188 | 8,390 |
| Static entity pass wall, ms/frame | 1.071 | 1.082 |

The native dynamic/static rows use complete log windows. Both walk
histograms give p50/p95/p99 bounds of [75,100), [100,150) and [150,250) ms.
The time-matched 20–150 s segments average 12.01 and 12.28 FPS.
At 115–120 s, the aggregate client `entities` span averages 55.01 and 36.54
ms/frame. That span includes early dynamic and static work. Its
dynamic submit lap falls from 8.94 to 7.03 ms/frame in the full capture.
The heaviest complete dynamic window in each run falls from 53.93 to 34.40
ms/frame, 678 to 291 draws and 679 to 292 texture calls at 24.6k and 23.4k
submitted vertices. The static path changed in this build. Its measured
cost is stable, so the change in dynamic submission needs separate
attribution. The two runs show how strongly route time can respond to
packet fragmentation at comparable geometry volume.

Standing averages 36.406 FPS with a 50.533 ms slowest-1% mean. At 15–25,
30–80 and 95–105 s the latest FPS is 43.2, 31.2 and 43.1. Process CPU is
1.274, 1.883 and 1.231 logical CPUs. Solid and translucent keep 43 and 26
indexed draws/frame and 24,624 and 20,084 submitted vertices/frame across
complete windows. Dynamic and static entity draws are zero. The earlier
thread CPU capture identifies one additional busy thread during this
plateau. The executing stack on that thread remains open. Win7 requires
an external stack capture because the built-in sampling profiler is Linux
only. A short Very Sleepy capture during the plateau can be resolved
against the matching build's module bases and COFF symbols.

At upstream `f037744a79f9982c47876f49c5a3067279f88706`, 370 commits beyond
the integrated source, terrain view distance, block-entity range/frustum
selection and particle geometry still use separate filters. The
block-entity and terrain range functions and the shared particle geometry
function give identical source hashes at the two revisions. The changes
to item frames in this range concern the break hook. An object drawn beyond
the terrain boundary can still come from a live entity or resident column.
The screenshot shows this boundary, while actual column unload requires a
position and lifecycle trace.

The `dx9-dev` atlas candidate is
`b1a03b1588a347542cd4a40ae0802613d6e7c898`. Upstream wgpu already shares a
single 2D texture and large indexed material runs for ordinary entities.
The DX9 change reuses its own virtual atlas texels, tile padding, mapping
and ordered quad stream. A qualifying dev SWVP pass has at least 16 adjacent
same-state splits caused by shared-atlas pages. Later dynamic passes try
one extra 2048-square managed texture if its 16 MiB fits the budget and
caps. A 128px tile with gutters allows 225 slots. Tile refreshes feed both
placements from the same source. The separate layout keeps static and HUD
UVs valid. Sprite and foil order, alpha/depth, the vertex payload and shader
paths follow their existing rules. Setup needs a temporary 16 MiB blank
image for the new texture. The extra page and its bounded tile metadata
are retired on capacity, resource allocation failure or budget pressure.
Device loss uses the existing recovery and pending-patch retry. Placement
failure remaps the pass before drawing it.

The new log reports ordinary, sprite and foil ranges plus adjacent page
breaks and active compact tiles. Compare the new walk against `2129b7999a`,
especially the 100–125 s segment. Use actual dynamic indexed draws,
texture calls, NumVertices and submit wall. Check the first atlas fill
for a frame-time spike. The CPU test covers exact tile seam coordinates,
a patch queued for both layouts and deterministic fallback on a full page.
Visual acceptance includes player skins, signs, particles and glint, followed
by resize, resource reload, GMA timing and HD4570 hardware smoke.

[Win7 package run 44905517691](https://github.com/Mamsikas/horizon-legacy/actions/runs/44905517691)
passed the build and import audit, import-policy tests and the CPU resource
and embedded SM2 checks for both targets. The ZIPs contain build metadata
and executable checksums.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14708930300](https://github.com/Mamsikas/horizon-legacy/actions/runs/44905517691/artifacts/14708930300) | `0a18ba47fa9530eafdaeb9e99a531bdb228c698d7b0922f1f7314651fba93eb5` |
| x86 | [14708935101](https://github.com/Mamsikas/horizon-legacy/actions/runs/44905517691/artifacts/14708935101) | `cf8dc5fce08bd4a51a3b2232b6f505d8f268d15b2124b1d329b710c5b9207573` |

## Atlas capacity result and standing cause, 2026-11-22

The owner re-ran the previous build before the new build on the same route.
`walk_previous.hzperf` has 1,798 frames in 152.836 s. `walk.hzperf` has
1,767 in 150.775 s. The rotated log identifies the new capture as clean
`b1a03b1588a347542cd4a40ae0802613d6e7c898` on Win7 x64, Chell 1.8b,
GMA3150 HAL SWVP. The older capture's build comes from the owner.
Neither capture embeds a commit. Both frame histograms give p50/p95/p99
bounds of [75,100), [100,150) and [150,250) ms.

| GMA walk metric | Previous build | Initial compact page |
| --- | ---: | ---: |
| FPS, full capture | 11.764 | 11.719 |
| DX9 renderer wall, ms/frame | 61.038 | 60.189 |
| Entities span wall, ms/frame | 14.752 | 13.336 |
| Slowest 1% mean frame time, ms | 198.30 | 245.45 |
| 20–55 s FPS | 11.686 | 12.343 |
| 20–55 s DX9 renderer wall, ms/frame | 60.115 | 54.299 |
| 20–55 s dynamic entity submit, ms/frame | 9.722 | 4.632 |
| 115–120 s FPS | 8.8 | 7.6 |
| 115–120 s entities span wall, ms/frame | 42.621 | 55.897 |

The compact texture enabled 14.8 s before the capture after 17 adjacent
same-state page breaks. It had 214 of its 225 padded tile slots at 51.2 s
and reached capacity at 57.2 s. The remaining route used the shared atlas.
The late block exercised the ordinary submit path. Its page assignments
can still differ from a walk that used the shared atlas from the start.
In the new log's complete windows before exhaustion, the dynamic pass makes 10.11
indexed draws and 8.74 texture calls/frame at 8,784 indexed vertices/frame.
Complete windows after exhaustion make 156.60 and 156.22 at 8,066 vertices.
The scene changes across those groups. The log shows 838 compact passes
including warm-up, zero sprite batches during the recorded walk, and zero
aborted frames in the 1,712 frames covered by complete log windows.
The layout adds useful coverage while active. The full-route result and
frame-time tail fail the current acceptance criteria.

The follow-up keeps one managed 2048-square A8R8G8B8 texture. This is
16 MiB of texture payload and the tested device's largest single page.
With 130px padded cells, 1024-square fits 49 tiles and 2048-square fits
225. The 16 MiB describes the optional resource size. The renderer keeps
its existing advisory total texture budget and releases the page under
pressure. Slots absent from both the current and previous frame can now
be reassigned to new tiles, oldest use first. Every logical tile that
cannot be admitted is mapped through the shared layout in polygon order.
The admission budget is 512 KiB/frame across both dynamic phases, or up
to seven 130px cells. Existing dirty tile updates and the one-time
resource initialization are separate. Geometry, filtering and draw
state keep their existing order. Only a resource failure remaps the
entire pass. Device loss still uses the existing retry path.

The updated native log adds recycled slots and compact placement misses.
It counts repeated tile uses in the latter, so it is not a distinct-tile
count. Check that compact passes continue after 57 s and compare draw,
texture, upload, whole-route tail and visual results on GMA. CPU tests
cover both layouts at a tile seam, dirty updates, main-atlas fallback,
current/previous-frame protection and removal of a recycled tile's
stale upload. The HD4570 production path still follows its own caps.
No new hardware speedup is claimed for the recycling candidate.

### Standing root cause

In `standing_vs.hzperf`, the external stack profiler process ran from
24.8 to 70.3 s. At 30–55 s the game falls to 4.16 FPS, with about one
logical CPU consumed outside Horizon. At 75–80 s after the profiler the
game is still around 30.4 FPS with roughly 1.91 logical CPUs spent in
Horizon on the plateau. At 90–105 s it runs 43.8 FPS at 1.24 logical CPUs.
Very Sleepy samples all threads by suspension and stack walking. Its
10% rate controls the pause between scans. Its overhead on N455 makes
the profiled frame times unsuitable for a normal benchmark.

The 35.084 s stack file contains 4,321 samples. A thread whose root is
`VanillaTerrain::search_rings_behind` has weighted stacks in
`RingSearch::find`, climate/biome sampling and Perlin noise for about
87.8% of its thread sample weight. The remaining stacks for that worker
show its sleep loop. The DX9 compactor's sampled stack is a channel wait.
These are weighted snapshots and the format has no per-thread IDs.
The result identifies the background calculation of stronghold positions
that matches the prolonged CPU plateau from the earlier thread accounting.
The worker runs outside world ticks and may start after a quiet generation
interval, or after 60 s from creating the terrain context. Freezing world
ticks does not stop it. It is shared world generation, outside the DX9
work in this fork.

### Win7 packages for tile recycling

[Run 45006402367](https://github.com/Mamsikas/horizon-legacy/actions/runs/45006402367)
built clean `f75bb8c4002e709029fd4f4ea04d8f7736e06d61` for both targets.
The Win7 import audit, import-policy tests and CPU resource/embedded SM2
checks passed. Local renderer compile checks passed on both targets with
and without dev instrumentation, including the x64 test compile. Both
atlas tests passed in the CPU harness. A synthetic 400-tile route also
stayed on one page after filling 225 slots and within seven new tiles
per frame, with no COM work in the harness.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14748477272](https://github.com/Mamsikas/horizon-legacy/actions/runs/45006402367/artifacts/14748477272) | `2accc7537f9c747fb34a4458f593d8b549eeb27a056b72f3450ad6b3f615f022` |
| x86 | [14748666368](https://github.com/Mamsikas/horizon-legacy/actions/runs/45006402367/artifacts/14748666368) | `a8b15e033b5deacb26100de626b61b901e83b60ce18510241fe4bb1299051e1a` |

## GMA tile reuse and first atlas fill, 2026-11-22

`profile-20261122-094047Z.hzperf` contains 2,041 frames in 156.578 s.
The accompanying log identifies clean
`f75bb8c4002e709029fd4f4ea04d8f7736e06d61`, Chell 1.8b and GMA3150
HAL SWVP on Win7 x64. The two earlier November 22 walks are the references.
The older recordings use the same route. Scene work can vary between runs.

| Walk metric | Shared atlas | Compact page without reuse | Compact page with reuse |
| --- | ---: | ---: | ---: |
| FPS, full capture | 11.764 | 11.719 | 13.035 |
| DX9 renderer wall, ms/frame | 61.038 | 60.189 | 53.690 |
| Entities wall, ms/frame | 14.752 | 13.336 | 9.831 |
| Dynamic entity submit, ms/frame | 8.457 | 7.690 | 4.202 |
| Frame p50 / p95 / p99, ms | 82.18 / 132.04 / 158.26 | 79.06 / 141.93 / 201.31 | 72.12 / 122.83 / 201.10 |
| Slowest 1% mean, ms | 198.30 | 245.45 | 258.40 |
| 100–125 s FPS | 9.08 | 8.72 | 11.12 |
| 115–120 s entities wall, ms/frame | 42.62 | 55.90 | 20.83 |

The p50/p95/p99 values use nearest rank. Slow frame events cover all
frames above 50 ms in these three captures. The event IDs are distinct and
the counts match every histogram bucket above the threshold. That allows
these percentiles to be resolved within the buckets.

The page activated near 2.7 s in the reuse run and stayed available
through the end. The last complete native log window has 225 occupied
slots, 2,197 compact passes and 28 recycled slots, with the first
recycling seen around 144 s. The working set grew more slowly in this run.
The 100–125 s result shows the value of retaining compact submission
before recycling was needed. Across complete native windows within this
section the no-reuse and reuse runs have 18,052 and 18,237 indexed
vertices/frame. They make 412.89 and 9.70 indexed draws/frame, 412.42 and
7.05 texture calls/frame, and spend 35.79 and 16.81 ms/frame in the native
dynamic entity pass. The texture write volume is 56.83 and 0.63
KiB/frame. No GPU timestamps were collected.

The native window ending around 7.7 s wrote 20.51 MiB in the dynamic
entity pass. The bytes account for the 16 MiB zero fill plus seventy
padded tile updates. One frame at 3.8 s has 116.8 ms in the dynamic submit
span. The window locates the first texture fill alongside that frame.
The exact time inside creation/clear needs its own span. The worst 1%
still needs work, including stalls elsewhere in the renderer and tick
path. The recycling candidate remains in dev for follow-up testing.

The next dev build clears the optional page directly in the managed
D3D9 texture lock. It retains the 2048-square page, format, transparent
initial bytes and padded tile placement. This removes the temporary
16 MiB allocation and its RGBA-to-BGRA pass. Driver creation, LockRect,
writes to the texture and the actual tile uploads still have costs.
Ordinary texture data and the existing fallback use the same write
rules as before. The `d3d9_entity_page_init` span measures creation
and clear on the one frame that allocates the page. The CPU byte test
guards tightly packed and padded locks. Use a new cold walk and its log
to measure this one-off change and check that steady-state coverage
and the heavy section remain comparable.

### Win7 packages for direct page initialization

[Run 45009656070](https://github.com/Mamsikas/horizon-legacy/actions/runs/45009656070)
built `39dd48f04f7fe560847ccaae56cafc5503a0b2e2` for Win7 x64 and x86.
Import audits, policy tests and CPU resource and embedded SM2 checks
passed. Local Win7 renderer compile checks passed in both ordinary
and dev modes on both targets. The x64 check included the DX9 locked
row test target. That test was also run with a CPU buffer for tight and
padded pitch. Hardware timing for the direct clear remains pending.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14750265428](https://github.com/Mamsikas/horizon-legacy/actions/runs/45009656070/artifacts/14750265428) | `706d6b8d9abc994bfd9e7ea0db5d0894a39913e91b0260812535484bbf9f7fc0` |
| x86 | [14750130891](https://github.com/Mamsikas/horizon-legacy/actions/runs/45009656070/artifacts/14750130891) | `c9d09a784fdf35a48363144da51bf3240d6ab5c80678ba8715d02d6515ea9c64` |

## GMA direct-clear captures and solid terrain priority, 2026-11-22

`walk (2).hzperf`, `standing (2).hzperf` and `latest (18).log` identify the
clean `39dd48f04f7fe560847ccaae56cafc5503a0b2e2` build on Chell 1.8b,
GMA3150 HAL software VP, Win7 x64, 1024×600 and RGB565. Walk has 1,915
frames over 150.876 s. The standing control has 4,435 frames over 122.611 s.

| Walk metric | Shared entity atlas | Compact page with reuse | Direct clear with reuse |
| --- | ---: | ---: | ---: |
| FPS | 11.764 | 13.035 | 12.693 |
| DX9 renderer, ms/frame | 61.038 | 53.690 | 54.790 |
| Solid, ms/frame | 32.182 | 31.251 | 31.909 |
| Entities, ms/frame | 14.752 | 9.831 | 9.492 |
| Dynamic conversion / submit, ms/frame | — / 8.457 | 4.129 / 4.202 | 4.407 / 4.038 |
| Frame p50 / p95 / p99, ms | 82.18 / 132.04 / 158.26 | 72.12 / 122.83 / 201.10 | 73.69 / 126.98 / 191.69 |
| Slowest 1% mean, ms | 198.30 | 258.40 | 236.74 |
| 100–125 s FPS | 9.08 | 11.12 | 10.64 |

The compact page activated 10.5 s before the new walk and reached 225
slots. The initialization span falls outside the capture. The walk
confirms the page stayed active after direct initialization. Its steady
state and mixed differences against the reuse run give no timing for
the one-off clear. The USHORT indexed draw counts and primitive bounds
come from 29 complete native log windows over 1,853 walk frames and 23
windows over 4,130 standing frames. All selected windows have zero
aborted frames and zero recorded failures. There are no GPU timestamps.

| Scene / native pass | Wall ms/frame | Indexed draws/frame | Sum NumVertices/frame | SetStreamSource/frame | SetTexture/frame |
| --- | ---: | ---: | ---: | ---: | ---: |
| Walk solid | 31.776 | 285.97 | 86,185 | 45.06 | 5.05 |
| Walk translucent | 3.429 | 18.45 | 9,884 | 9.87 | 13.71 |
| Walk dynamic entities | 8.887 | 6.00 | 8,602 | 1.11 | 4.75 |
| Walk static entities | 1.035 | 4.65 | 2,098 | 4.37 | 2.25 |
| Standing solid | 10.694 | 44.00 | 24,624 | 4.00 | 2.00 |
| Standing translucent | 8.387 | 26.00 | 20,084 | 0.00 | 23.00 |

Native pass boundaries differ slightly from the capture spans. In the
capture every 5 s walk interval has more wall time in solid than in
entities. Entities reach an average of 27.72 ms in the 115–120 s interval.
An individual frame at 51.441 s has 82.93 ms of entities, including
78.41 ms of dynamic submit, with 33.52 ms of solid. Those peaks still
matter. Average solid is about 58% of DX9 wall. The regular path builds
no index buffers in these windows, so the improvement target is the
calls or their vertex and pixel workload. Time in the pass includes waits.

Standing averages 36.171 FPS against 36.406 in the earlier logged control.
The new 30–80 s interval averages 31.32 FPS, followed by 43.73 FPS from
90–105 s. The older control averages about 31 and 43 FPS in those phases.
Both have p50/p95/p99 bounds of [25,30), [30,40) and [40,50) ms. The new
slowest 1% mean is 69.04 ms versus 50.53 ms. The longest burst around
84.3 s is in solid and translucent at the same submitted geometry
counts. The entity page was absent throughout standing. The cause of
this short burst needs separate evidence from the previously sampled
worldgen plateau.

The integrated Horizon wgpu path and upstream `f037744a79f9982c47876f49c5a3067279f88706`
reuse an entity stream partitioned by material. `EntityBatch::update`
copies material runs into that stream, and the ordinary wgpu run goes
through one indexed draw when there are no prepared model draws. Static
block entities are cached by section revision. DX9 already borrows the
long material runs, shared texture page and static section cache. The
new route leaves several ms each in DX9 conversion and submission. It
supports focusing on terrain while preserving entity caching and order.
In `SectionLists::draw` wgpu binds a common VB/IB pair for a run and can
use indirect multidraw. The DX9 candidate uses the common binding and
tight indexed ranges supported by the existing HAL device.

### Dev SWVP dense solid windows

This experiment is active in dev SWVP. Each existing solid material,
atlas, arena and fog batch starts with its same canonical range list.
If the list repeats, the renderer tries a single immutable SYSTEMMEM
INDEX16 buffer for it. Windows keep the order of the original indices.
A joined window allows at most 32 vertices in each gap, 2,048 vertices
in its span, and 1/16 additional vertices over the referenced count.
Singletons keep their original bounds. The trial requires at least
three saved draws per buffer to cover up to two new `SetIndices` calls.
Sparse plans and failed optional IB allocations use the shared quad
indices. Device loss follows the existing recovery path.

Only the used indices are copied. Cache retention is limited to the
lesser of 4 MiB and 1/16 of the configured mesh budget, including draw
window metadata. Range copies reuse per-batch CPU vectors. Worker mesh
data, visibility, vertex buffers, cutout tests and fog classification
follow the same rules. An exact range change or inactive bucket drops
its cached buffer, invalid arena IDs are removed on terrain preparation
after Reset, and world and resource rebuilds clear the batch table. The
ordinary SWVP and HW VS2 paths keep their existing strategies.

`geometry.rs` builds and tests the ordered windows, `device.rs` submits
them with one IB binding, `submit_profile.rs` reports `solid_draws_saved`
and `solid_extra_vertices`, and `legacy/mesh.rs` with `legacy/frame.rs`
handles visibility-key and budget reuse. Native `Solid` names the
index upload bytes, `SetIndices` and actual draw and vertex counts. For
these counters the same-frame reference is `dip_ranges + solid_draws_saved`
and `dip_num_vertices - solid_extra_vertices`. That identifies whether
the experiment activates even when scene workload shifts. Reduction in
COM calls needs to pay for extra vertex processing and any IB uploads.
Compare the same walk and standing with the `39dd48f04f` captures and
review fog edges, foliage cutouts and resize. Visual tests, timing, and
smoke on GMA3150 and HD4570 require native hardware.

### Win7 packages for dense solid windows

[Run 45014529557](https://github.com/Mamsikas/horizon-legacy/actions/runs/45014529557) built
`31d02267b6eb8d2451e44fcd3699d6b7fe9a1f33` for both architectures. The
Win7 import audits, 15 import policy tests per architecture and CPU
resource and embedded SM2 checks passed. The local Win7 compile checks
covered ordinary and dev modes. The local CPU suites passed 33 tests
with diagnostics and 31 without them. The new tests preserve quad
order and tight sparse ranges and check the cap and byte-budget fallbacks.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| x64 | [14752245486](https://github.com/Mamsikas/horizon-legacy/actions/runs/45014529557/artifacts/14752245486) | `4aa413ec6265ffb0cc282143732153b515348f5d5286e68f868263db017fac3f` |
| x86 | [14751064388](https://github.com/Mamsikas/horizon-legacy/actions/runs/45014529557/artifacts/14751064388) | `7fb8c7fc6942762c6692e7c38e50f17c5b85a6d18f7b9c25bf8a996b38e40be1` |

### GMA result and rollback

The log for `profile-20261122-112410Z.hzperf` (walk) and
`profile-20261122-112126Z.hzperf` (standing) identifies clean
`31d02267b6eb8d2451e44fcd3699d6b7fe9a1f33`, Chell 1.8b, HAL SWVP, Win7
x64 and 1024×600 RGB565. The unchanged `39dd48f04f` captures above serve
as the same-route reference. Wall times include driver waits.

| Capture metric | Reference | Dense windows |
| --- | ---: | ---: |
| Walk FPS | 12.693 | 12.514 |
| Walk DX9 wall, ms/frame | 54.790 | 55.667 |
| Walk solid wall, ms/frame | 31.909 | 32.487 |
| Walk frame p50 / p95 / p99, ms | 73.69 / 126.98 / 191.69 | 74.65 / 124.73 / 165.30 |
| Walk slowest 1% mean, ms | 236.74 | 218.54 |
| Standing FPS | 36.171 | 35.882 |
| Standing solid wall, ms/frame | 10.521 | 10.535 |
| Standing slowest 1% mean, ms | 69.04 | 88.24 |

The same-frame counters show 4.94 draws saved per walk frame from 286.60
original draws, 76.76 extra vertices in the advertised SWVP ranges and
2.24 `SetIndices` calls. The pass built 201 index buffers across 1,825
frames in 29 complete windows, about 3.30 KiB of index writes per frame.
Standing saved 6 of 44 draws, added 64 vertices and introduced 3 index
bindings per frame. There were no recorded COM failures. Frame tails
moved in both directions across the two scenes. Solid wall did not
improve and the owner saw no speedup, so the experiment was reverted.

Across the complete log windows the main thread used about 92.9% of
walk elapsed time (74 ms/frame), compared with 92.0% on the reference.
The sparse solid samples totaled 874 ms of thread execution in 904 ms
of wall time over 28 samples. Windows accounts CPU time in coarse ticks
here. Together these measurements support main-thread CPU pressure on
the one-core N455, including software VP, driver and general client
work. GPU time, rasterization cost and GPU queue stalls remain unmeasured.

## November 22 upstream sync and solid face bounds

The merge pins Horizon `10b9f3472770ed9307e3d92a509d753f94a03a42`,
564 upstream commits after `df50a07e73`. The first review used `45eeccded1`.
The latest pin adds eight commits after that review. The fork retains
its Windows 7 ABI and client distribution workflow.

Several measured upstream changes run on both renderer paths. `82e61665c4`
speeds construction of the solid/fluid `SectionMesh` and its face ranges.
`a32fc120ee` uses immediate section meshing for block edits. `0f0dea6114`
changes the shared streaming upload budget. `d939fd378d` reuses shared
chunk lookups, `e349aeccea` changes light import and propagation,
`e095541dc6` recycles incoming chunk decode scratch, and `ad406095d4` and
`b45d73de43` reduce shared particle and weather preparation. `0c2cff4f46`
updates the `Resident::walk` and occlusion structures that DX9 already
calls. Its `PageRuns` portion is specific to the wgpu renderer. Upstream's
FPS and memory results cover other machines and settings.

The unchanged-uniform and GPU animation work in `8e0b8ff5a5`, the wgpu
entity stream in `e0c1233a27`, the GPU arenas in `5fc0fdfdb9`, and
visibility buffers, compute culling and mesh shaders in `a27fee23ca` and
`45eeccded1` all have separate backend contracts. DX9 keeps its CPU
traversal and software-vertex HAL path.

The face-bound trial borrows the bounded sub-section rejection from
upstream meshlets. DX9 calculates the bounds of its existing seven solid
face groups once at section conversion, in the same region coordinates
sent to the device. That is at most 168 bytes of bounds per resident
solid section. Dev SWVP tests groups only on the edge of the frustum.
A group is rejected only if its expanded bound lies fully outside the
projection. Unknown bounds and cold sections keep their original draw
path. The exact visible group mask joins the existing solid plan stamp,
so camera motion restores a group as it re-enters. The cache uses the
same revisions and arena/Reset identity as before.

Alpha order, materials, visibility distance and all translucent geometry
keep their existing routes. This trial creates no new per-frame GPU
buffers or index writes. It is a separate commit from the integration
and visual fixes, so a comparison on GMA can measure it independently.
The dev log reports `solid_culled_buckets`; compare that with the real
`Solid` `dip_num_vertices`, draw counts and wall time. Check screen edges,
caves, cutouts and resize while holding the route and settings constant.

### Win7 build compatibility

The [first package run](https://github.com/Mamsikas/horizon-legacy/actions/runs/45034266810)
linked both clients and then failed their mandatory import audit. Upstream
`5115507c83` uses four Windows 10 CPU-set functions to reserve a physical
core for the server on machines with enough SMT cores. The Win7 target now
resolves the four functions together on demand. Without the full API the
OS keeps scheduling all threads under the existing affinity. The other
Windows target keeps using the SDK imports directly.

The import policy still rejects all four static and delay imports. The
audit also verifies the newly linked directory enumeration, volume,
processor group and ACL imports individually against Windows 7 or earlier
exports. There are no workflow changes.

### Win7 packages and GMA comparison

Both builds contain the upstream sync, the Win7 compatibility fix and the
DX9 visual corrections for lava, fire, honey and the mob hurt overlay.
The code for the terrain trial lives in `legacy/mesh/convert.rs` and
`legacy/mesh/compact.rs`, which calculate bounds during the existing
conversion. `legacy.rs` enables it for dev SWVP. `legacy/mesh.rs` owns
and accounts for the bounds. `legacy/solid_cache.rs` stores the mask
in its stamp and `legacy/frame.rs` skips the rejected packets.

The local x64 control A was built at
`52617f2717c17fd7d6edf2f88051df17c7914633` with source tree
`78404445c77c4154228fa7d2dd5e68eee597aa33`. Reproduce that tree by
cherry-picking `2203e06b145335af37ef6c4993bc14ed2b1dcad3` onto
`73b8ef27183adea3fea286223f677daff1ad1fb7` and run the standard Win7
client build with `WIN7_D3D9_DEV=1` for the detached checkout.

[Run 45149225385](https://github.com/Mamsikas/horizon-legacy/actions/runs/45149225385) built the downloadable
x64 control at `9b88f7f95204e777498c747ace0a2836f30bb2ff` from that identical tree.
The package passed the Win7 import audit and embedded SM2 checks.

[Run 45036840020](https://github.com/Mamsikas/horizon-legacy/actions/runs/45036840020)
built candidate B at `2203e06b145335af37ef6c4993bc14ed2b1dcad3` for
both architectures. Both jobs passed the executable import audit,
the 15 import-policy tests and the CPU resource and embedded SM2 checks.
The A archive passed the same executable audit and its internal SHA256
manifest was verified. Both use the pinned toolchain and the legacy
ThinLTO profile. Local D3D9 dev tests and the extracted CPU tests for
terrain alpha, honey ordering and frustum edges passed. FXC validated
both new hurt-overlay programs as PS2.

| Package | Archive | ZIP SHA256 |
| --- | --- | --- |
| A x64 | [14806701989](https://github.com/Mamsikas/horizon-legacy/actions/runs/45149225385/artifacts/14806701989) | `2d0aef4f4139930a9561641eb06cb7c6d8b093791a94adca537108651b6d28ed` |
| A x64 (local) | Local build | `04d8001fa3bf29c3d667f317fa2e2f67d35e7dca4ce61095cb945160a8e753c4` |
| B x64 | [14759944541](https://github.com/Mamsikas/horizon-legacy/actions/runs/45036840020/artifacts/14759944541) | `9068a4ccdf52d1930e5f527ec84652ba6b81231494ac29d46040fc8210934a4a` |
| B x86 | [14760517328](https://github.com/Mamsikas/horizon-legacy/actions/runs/45036840020/artifacts/14760517328) | `8db68603dec4ecde7fa90800186a42732740be753ddd35d5dd256e36d35aeec7` |

Use the same server route, window, quality and warmup for A and B. Keep
the default dev diagnostics and record the chat client profile and
`latest.log` for each build. Compare the solid draw and vertex totals
against renderer and frame times. Use standing as a regression control.
Native GMA3150 and HD4570 execution, visuals, Reset and timing await
hardware results. Review the four corrected effects and the terrain
at the window edges during the hardware smoke test.

### GMA face-bound result, 2026-11-23

The four captures and both logs identify clean `9b88f7f952` (A)
and `2203e06b14` (B) on GMA3150 with Chell 1.8b, HAL SWVP, PS2 and
1024×600 RGB565. The recorded settings match within each pair. The owner
confirmed lava, fire and the mob hurt overlay. Honey still differs
visually and is deferred to the translucent pass work.

| Metric | A | B |
| --- | ---: | ---: |
| Walk seconds / frames | 153.358 / 2147 | 150.108 / 2083 |
| Walk FPS | 14.000 | 13.877 |
| Walk DX9 wall, ms/frame | 53.369 | 53.580 |
| Walk solid wall, ms/frame | 31.754 | 31.327 |
| Walk frame p50 / p95 / p99, ms | 69.10 / 107.13 / 125.08 | 68.48 / 108.23 / 137.18 |
| Walk slowest 1% mean, ms | 179.36 | 188.77 |
| Standing FPS | 37.345 | 37.687 |
| Standing solid wall, ms/frame | 10.078 | 9.911 |
| Standing frame p50 / p95 / p99 buckets, ms | [20,25) / [30,40) / [40,50) | [20,25) / [30,40) / [40,50) |
| Standing slowest 1% mean, ms | 57.69 | 53.89 |

Across 29 A and 28 B whole walk log windows, 2,049 and 1,964
frames respectively, solid submits fell from 283.50 to 266.16 indexed
draws/frame and from 85,881 to 82,274 advertised vertices/frame.
These numbers describe the actual submitted work in each run. The
camera and visible geometry differ between the route samples. Standing
has 23 whole log windows per build. Its counts were stable at 43 to
42 draws and 24,624 to 24,512 vertices. There were zero aborted frames
and zero recorded COM failures in these windows. GPU timestamps are
absent.

Over the common first 150 seconds of walk, FPS was 13.993 and 13.873.
Solid draw wall fell 31.569 → 31.321 ms/frame. The separate indices and
cold-draw stage rose 0.134 → 0.231 ms/frame. The last-frame log snapshots
reused the solid plan in 11/29 and 2/28 windows respectively. Camera
motion can change the group mask and rebuild a plan. The measured
increase in plan time is smaller than the change in solid draw time.
Entities rose 9.330 → 9.558 and GUI draw 4.752 → 4.870 ms/frame.
Tick and preparation also rose. Solid improved in some matched route
intervals and was slower in others. The slowest 21 frames in each
common interval spent on average 41.72 → 40.90 ms in solid and
13.82 → 17.02 ms in preparation. These captures do not attribute the
frame tail or FPS change to culling alone.

The steady standing counts show the mechanism works, but the mixed
route times and frame tail do not establish a useful overall gain.
The face-bound code was reverted. GitHub control A is the reference
for the next isolated trial. Solid took more wall time than entities in
all thirty full five-second walk blocks, keeping it the priority.

### Pack compatible solid ranges during existing arena compaction

Upstream was reviewed at `2e7d2c6bb6f533224df1c4dfaf67548b9bd2c7c9`,
288 commits past the integrated pin. `eee912b040` clusters terrain
by locality so the GPU can process smaller, tighter meshlets. It also
keeps the relative emission order of overlapping quads. DX9 applies
the same locality principle to the existing submission batches.
`1fd5f925a8` reduces shared mesher and ambient occlusion preparation.
`d8f558197d` requires the modern compute and depth pyramid path.
The dev trial uses the current shared client revision.

Before compaction, vertex data is laid out section by section.
Different atlas pages, cutout states and translucent tails can
separate opaque draw ranges that share one DX9 state key. A resident
compaction job on dev SWVP now tries to put compatible ranges side
by side in its existing replacement arena. The layout is stable
within each page and cutout state. Per-section source indices, facing,
fog selection and translucent emission order remain valid. The
existing batch merger can make one tight draw out of adjacent visible
ranges. Any skipped geometry still leaves a gap between draws.

`mesh/compact/materials.rs` validates and packs up to 512 ranges.
The extra worker payload is one encoded arena of at most 2 MiB.
It uses the existing single compaction job and page-size bound.
Jobs with invalid, excessive or already contiguous ranges use the
original layout. `mesh/compact.rs` keeps the same revision checks and
allocates the replacement VB before releasing the old one. The range
mapping lives with the resident arena, with the original compressed
shadows available for eviction, reload and Reset. `mesh.rs` and
`frame.rs` look up the section's sorted source slices. Arrivals and
demand uploads keep the original layout. Hardware VP uses the existing
compact vertex and upload path.

The log reports `solid_material_packed_arenas`. A useful result should
show fewer solid indexed draws at comparable submitted vertex counts
and a lower solid wall time on the same route. Compare the sum of
terrain cache, indices/cold draws and solid submission as well as the
whole frame and its tail. Check cutouts, terrain edits and resize on
the native hardware. The transparency issue remains a separate test.

### Windows 7 packages for resident material packing

[Run 45160459036](https://github.com/Mamsikas/horizon-legacy/actions/runs/45160459036)
built candidate C at `ef6b3bf914bce657e797b87e6fd053ce98c419c0`.
Both jobs passed the executable Win7 import audit, the 15 import-policy
tests and the CPU resource and embedded SM2 checks.
Local Win7 dev client checks passed for both architectures. The ordinary
x64 client and Win7 renderer test targets compiled. The host D3D9 dev
suite passed 32 tests. The host harness ran the two packing tests with
the real encoded vertex format and checked 32,768 combinations of
visible packets and fog states. It recovered the same bytes and
primitive order per draw group with no increase in draw count.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| C x64 | [14813745736](https://github.com/Mamsikas/horizon-legacy/actions/runs/45160459036/artifacts/14813745736) | `59fb023de2764d367d5bed5a6190299d57df92fec034816a159bfd48e4153f06` |
| C x86 | [14813847027](https://github.com/Mamsikas/horizon-legacy/actions/runs/45160459036/artifacts/14813847027) | `75b1ce82f2a5bfbc9f2d1a7682b361c8817c4414cf6ff3b2e6656242fa33444c` |

Compare C with the existing GitHub control A `9b88f7f952`. The captures
in this iteration provide the reference for the first hardware run.
Win7 static imports and CPU tests do not establish GMA FPS, visual
parity or Reset and HD4570 behaviour.

### GMA resident material result, 2026-11-24

The standing capture `profile-20261124-142211Z.hzperf`, walk capture
`profile-20261124-143159Z.hzperf` and `latest (5).log` identify clean C
`ef6b3bf914bce657e797b87e6fd053ce98c419c0`. Control A is clean
`9b88f7f95204e777498c747ace0a2836f30bb2ff`. Both runs used GMA3150 with
Chell 1.8b, HAL SWVP, FFP32, PS2, 1024×600 RGB565 and matching recorded
options and toolchains. Native call and vertex counts use complete
five-second log windows. Frame and pass times use the full captures.

| Metric | A | C |
| --- | ---: | ---: |
| Walk seconds / frames | 153.358 / 2147 | 152.893 / 2171 |
| Walk FPS | 14.000 | 14.199 |
| Walk DX9 wall, ms/frame | 53.369 | 51.995 |
| Walk solid wall, ms/frame | 31.754 | 29.988 |
| Walk solid indexed draws/frame | 283.50 | 160.88 |
| Walk solid indexed NumVertices/frame | 85,881 | 85,698 |
| Walk frame p50 / p95 / p99, ms | 69.10 / 107.13 / 125.08 | 67.13 / 108.21 / 153.02 |
| Walk slowest 1% mean, ms | 179.36 | 196.04 |
| Standing seconds / frames | 120.712 / 4508 | 142.609 / 5601 |
| Standing FPS | 37.345 | 39.275 |
| Standing solid indexed draws/frame | 43 | 30 |
| Standing solid indexed NumVertices/frame | 24,624 | 24,624 |
| Standing frame p50 / p95 / p99 buckets, ms | [20,25) / [30,40) / [40,50) | [20,25) / [30,40) / [30,40) |
| Standing slowest 1% mean, ms | 57.69 | 60.97 |

On the common first 150 seconds of walk, FPS was 13.993 → 14.180,
DX9 wall was 53.330 → 52.018 ms/frame and solid fell from 31.569 to
29.872 ms/frame. Solid improved in 26 of 30 five-second blocks.
Terrain cache rose 0.344 → 0.360 ms/frame and indices/cold draws rose
0.134 → 0.147. Entities were 9.330 → 9.558 ms/frame. The common first
120 seconds of standing reached 37.342 → 38.267 FPS, with solid at
10.088 → 9.812 ms/frame.

Twenty-nine full walk log windows per build covered 2,049 and 2,074
frames, with no aborted frames or recorded COM failures. C had
7–15 packed arenas in every window and four in each standing window.
Solid submitted 0.2% fewer advertised vertices with 43% fewer draws on
walk. There were no per-frame solid VB or IB uploads.
Measured main-thread execution across the walk windows was 66.57 →
65.05 ms/frame. GPU timestamps were absent.

On walk, C had 23 frames over 125 ms during seconds 15–30. A had none
in that interval. The p99 for the rest of the common 150 seconds was
127.72 → 125.72 ms. This split locates the main regression in the tail.
The 15–30 second interval shows higher solid, tick and prepare wall
times. Compactions continued in later windows with lower solid time.
The log aggregates five seconds of work, so it cannot assign the spikes
to individual jobs. Comparisons align by time from record start and
cannot guarantee identical camera positions. Keep C in dev for its
observed solid improvement and retest the walk tail before promotion.

### Reduce FFP shadow decode cost in resident material compaction

`geometry.rs` restores each 16-vertex byte plane and its XOR prefix in
one pass for compressed full FFP shadows in the existing dev SWVP
material-compaction job. Each byte keeps its prefix across blocks.
The worker uses 512 bytes of temporary plane storage and 32 bytes of
prefix storage. It retains the original encoded layout and leaves
the raw shadow copy and compact hardware formats on the existing path.
`terrain.rs` exposes the scoped append operation with the same checked
size, encoding and rollback checks.

`mesh/compact.rs` selects this operation for the existing material
plan, before packing, within the same single job and 2 MiB arena limit.
It measures wall time for appending and decoding the planned sections
when diagnostics are enabled. `frame.rs` reports the resulting
`material_compaction_decode` totals for collected jobs in the existing
five-second log. Fields record job count, restored bytes, sum and max
elapsed milliseconds. The interval excludes the material packing and
the owner thread's VB creation, and includes jobs later discarded on
revision mismatch. Draws, arena mapping, revision checks and lifecycle
use the same C code.

The 33 host D3D9 dev tests include exact float bit patterns, partial
blocks and compressed and pending raw shadows. The host packing
harness passed 32,768 combinations of visible packets and fog states.
A local Xeon Platinum 8573C benchmark ran the real append operations
on synthetic section shadows with pinned Rust, ThinLTO and SSE3/SSSE3.
The times include allocation, LZ4 decode and restoration into a
combined buffer. Each entry is the median of nine alternating rounds.

| Restored vertices | C, µs/job | D, µs/job |
| ---: | ---: | ---: |
| 2,048 | 90.66 | 46.41 |
| 8,192 | 352.54 | 169.05 |
| 65,536 | 2675.18 | 1155.98 |

Compare D to C on the fixed walk using the full frame distribution,
the 15–30 second interval, renderer and solid wall, and the new log
line. The host measurement covers only CPU shadow append. GMA frame
times and native visuals, resize, Reset and HD4570 await hardware.

Upstream review at `1a8910c3e9267a2f339f0d1db295ccdfa022f376` included
shared upload queue work in `cbac118119` and cave graph work in
`cb2132352e`. The modern shader changes in `d41da14ca2` target wgpu.
The integrated upstream pin for this experiment remains `10b9f34727`.

### Windows 7 packages for FFP shadow decode

[Run 45425376467](https://github.com/Mamsikas/horizon-legacy/actions/runs/45425376467) built candidate D
at `3f6bb578c2ea8e3b08491277c7010733a4ebe0d8` on `dx9-dev`. Both
architectures passed the Win7 executable import audit, 15 import-policy
tests and the CPU resource and embedded SM2 tests. Local Win7 client
checks also passed for dev x64 and x86 and ordinary x64.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| D x64 | [14938616921](https://github.com/Mamsikas/horizon-legacy/actions/runs/45425376467/artifacts/14938616921) | `47fec4f56d3559ab3bca09c7849a46aca67566c674f57ab8d37f97cbef7564d2` |
| D x86 | [14937711614](https://github.com/Mamsikas/horizon-legacy/actions/runs/45425376467/artifacts/14937711614) | `2a05a298888b63d7ee5ff74b0385c0809c7bf9e898476678d00e729caa64c1f1` |

Use the existing C x64 walk as the control and record the same route
with D x64. Keep the dev diagnostics and attach the client profile and
latest log. The C x64 package remains available from [run 45160459036](https://github.com/Mamsikas/horizon-legacy/actions/runs/45160459036/artifacts/14813745736).

### GMA FFP shadow decode result, 2026-11-25

`profile-20261125-085014Z.hzperf` is the walk and
`profile-20261125-084745Z.hzperf` is standing. `latest (6).log`
identifies clean D `3f6bb578c2ea8e3b08491277c7010733a4ebe0d8`, with
the same x64 toolchain, GMA3150 Chell 1.8b HAL SWVP, 1024×600 RGB565,
FFP32 and PS2 as C. The recorded options match for each scene.

| Metric | C | D |
| --- | ---: | ---: |
| Walk seconds / frames | 152.893 / 2171 | 152.692 / 2208 |
| Walk FPS | 14.199 | 14.461 |
| Walk DX9 wall, ms/frame | 51.995 | 51.432 |
| Walk solid wall, ms/frame | 29.988 | 29.637 |
| Walk frame p50 / p95 / p99, ms | 67.13 / 108.21 / 153.02 | 66.37 / 106.38 / 150.04 |
| Walk slowest 1% mean, ms | 196.04 | 201.67 |
| Walk solid indexed draws/frame | 160.88 | 159.10 |
| Walk solid indexed NumVertices/frame | 85,698 | 85,239 |
| Walk entities wall, ms/frame | 9.426 | 9.257 |
| Walk dynamic entity indexed NumVertices/frame | 8,715 | 8,555 |
| Standing FPS, common first 115 s | 37.974 | 35.870 |
| Standing solid indexed draws/frame | 30 | 32 |
| Standing slowest 1% mean, whole capture, ms | 60.97 | 76.90 |

Timing rows use whole captures unless stated. Draw and vertex rows use
whole native log windows. On the common first 150 seconds of walk, FPS
was 14.180 → 14.460 and solid draw wall was 29.872 → 29.521 ms/frame.
Control A was 13.993 FPS and 31.569 ms/frame for solid on that interval.
C and D had 23 of 185 and 25 of 200 frames over 125 ms during seconds
15–30. Both captures have their longest isolated frame near 39.3 s.

The entity stage accounted for 13.39% of walk frame time in both C and
D. The once-per-second entity counter had a median of 86 in both runs.
Dynamic submit nevertheless used 8,715 → 8,555 indexed vertices and
6.19 → 5.95 draws/frame. In the earlier A/B walk over 150 seconds,
entities rose from 9.330 to 9.558 ms/frame. A symmetric weighting of
the five-second blocks assigns 0.10 ms of that delta to the differing
frame counts across blocks and 0.13 ms to the times within blocks.
Blocks align by elapsed time. Visibility, geometry, draw state and
driver waits can vary within them.

D logged 135 planned material-compaction jobs in 29 full walk
windows. Decode appended 177.84 MiB of encoded vertices in 2.94 s of
elapsed worker time, averaging 21.78 ms/job with a 41.59 ms maximum.
This runs alongside the frame thread. C has no matching worker timer.
Standing had no collected material-decode jobs in its full windows,
with 24,624 advertised solid vertices/frame in both builds and four
versus five packed arenas. Two D standing frames took 614 and 430 ms
with renderer spans of 28 and 32 ms. The standing result records
both the lower mean FPS and the larger isolated spikes.

There were zero aborted frames or recorded COM failures in the
selected windows. GPU timestamps were absent. Retain the byte-exact
decode change in dev. The walk mean improved in this pair and the
slowest frames still need attention. The captures cannot isolate an
FPS gain from D's decode change. Native visual, resize, Reset and
HD4570 coverage is still open.

#### Sensitivity to route segments

Use the first 150 seconds and sum the three sibling terrain spans
`solid`, `cache` and `indices/cold draws`. The overall result divides
total stage wall time by total frames. Recalculate it ten times,
omitting the same 15-second segment from both captures each time.

| Pair | Delta, ms/frame | Improved segments | Delta range with one segment omitted, ms/frame |
| --- | ---: | ---: | ---: |
| A → B, face bounds | −0.143 | 7/10 | −0.505 to +0.083 |
| A → C, material packing | −1.669 | 9/10 | −2.377 to −1.444 |
| C → D, shadow decode | −0.348 | 8/10 | −0.400 to −0.291 |

The interval check tests dependence on a single segment within these
runs. Independent repeats measure run-to-run variation. Preserve both
candidate and control SHAs for close results. Judge those changes
using another paired run when the choice matters. Use per-pass draw
and vertex counts to verify the mechanism, and the full distributions
from both scenes to judge FPS and frame pacing. B's average terrain
effect is inconclusive. C's reduction in submitted draw calls also
appears at constant submitted vertices in standing. D changes worker
decode and reporting. Its entity-stage timings describe scene and
timing variation observed across C and D.

## November 25 long GMA repeats and RTX reload captures

Both GMA files named `D_gma_*.hzperf` belong to C
`ef6b3bf914bce657e797b87e6fd053ce98c419c0`. The client profile-start
times match the standing and walk in `D.log` at 16:13:40 and 16:17:20
local time. The log has one clean build identity. `С.log` identifies
the same C revision for the other pair. `B (2).log` identifies clean
B `2203e06b145335af37ef6c4993bc14ed2b1dcad3`. The subsequent
`realgmaD_stand.hzperf` and `realgmaD_walk.hzperf` match the profile
starts at 17:18:11 and 17:22:03 in `D_gma_real.log`. That log identifies
clean D `3f6bb578c2ea8e3b08491277c7010733a4ebe0d8`. All pairs are N455,
Chell 1.8b GMA3150 HAL SWVP, x64, 1024×600, RGB565 and the same
compiler/toolkit. The earlier short D pair remains valid.

| Metric | B | C | C repeat (files labelled D) | D |
| --- | ---: | ---: | ---: | ---: |
| Walk seconds / frames | 298.417 / 4253 | 299.523 / 4342 | 300.357 / 4351 | 299.301 / 4399 |
| Walk FPS | 14.252 | 14.496 | 14.486 | 14.698 |
| Walk DX9 wall, ms/frame | 52.224 | 51.234 | 51.056 | 50.439 |
| Walk solid wall, ms/frame | 30.510 | 29.623 | 29.631 | 29.268 |
| Walk solid indexed draws/frame | 266.31 | 161.89 | 159.63 | 159.68 |
| Walk solid indexed NumVertices/frame | 82,412 | 85,942 | 86,004 | 86,179 |
| Walk frame p50 / p95 / p99, ms | 68.09 / 102.69 / 117.45 | 66.42 / 104.00 / 128.51 | 66.58 / 103.69 / 135.91 | 65.43 / 103.88 / 121.70 |
| Walk slowest 1% mean, ms | 157.96 | 174.16 | 182.82 | 170.39 |
| Walk closing process RSS, MiB | 722.2 | 815.0 | 819.1 | 705.2 |
| Standing FPS | 39.958 | 40.009 | 41.521 | 38.672 |
| Standing frame p50 / p95 / p99 buckets, ms | [20,25) / [30,40) / [30,40) | [20,25) / [30,40) / [40,50) | [20,25) / [30,40) / [30,40) | [20,25) / [30,40) / [40,50) |
| Standing slowest 1% mean, ms | 49.03 | 52.17 | 48.28 | 49.91 |
| Standing closing process RSS, MiB | 660.2 | 571.0 | 566.1 | 734.1 |

On the common 295 seconds, solid was 30.423, 29.520, 29.511 and 29.179
ms/frame. The combined solid, indices/cold draws and terrain cache time
improved against B even when each 15-second segment is omitted in
turn. The two C runs have a 7.40 ms walk p99 difference on the same
binary. Standing differs by 1.51 FPS across those runs. B has a better
walk p99 and lower ending RSS in this set. Both C runs retain about
4 MiB of shadows and 27–28 MiB of buffers versus B's 4 and 27 MiB
at capture end. The recorded renderer allocations explain little
of the 93–97 MiB RSS difference.

The native counters cover 58 complete walk windows per build. They
record no aborted frames or COM failures. The two C runs took about
11.02 and 11.03 s of worker wall time across 3,823 and 3,858 staged
sections. Worker execution overlaps the game thread, which accounted
for 92.73% and 92.50% of elapsed time. That makes terrain preparation
worth reducing for N455 alongside the 29.6 ms solid submit stage.

D improves walk on both whole-capture mean and p99 against the two C
runs. Within the first 30 seconds B, C, C repeat and D have 11, 33, 35
and 20 frames over 125 ms. D's native walk windows contain 3,856
staged sections, 11.20 s of conversion wall and 271 collected
material-decode jobs. The latter total 6.03 s of parallel worker wall
for 357.84 MiB of restored bytes. The game's render thread was active
92.90% of elapsed walk time. The 68 solid samples total 1.76 s of
thread execution in 1.93 s of wall time, with coarse Windows CPU
accounting. D's standing has no staged sections
or material-decode jobs in its full log windows. Its lower mean FPS
and higher RSS in standing belong in the comparison and have no
direct worker-duration attribution. D remains the dev baseline.

### RTX 3050 and F3+A

The RTX DX9 capture is clean D `3f6bb578c2ea8e3b08491277c7010733a4ebe0d8`,
HAL hardware VP with compact vertex support, 1920×1080 RGB565. The
three wgpu captures are Horizon Nightly `f1bb66f353cf67f2aa4665293fc75c8d91186fc7`,
25 commits after the fork's shared pin. The logs place F3+A at about
three seconds. The capture metadata records profiler options. Full
graphics settings and identical game binaries are unverified.

| Metric | DX9 D | OpenGL | DX12 | Vulkan |
| --- | ---: | ---: | ---: | ---: |
| Full capture FPS | 173.09 | 256.32 | 364.07 | 372.79 |
| FPS over seconds 20–60 | 160.50 | 259.70 | 371.30 | 379.07 |
| Renderer wall over seconds 20–60, ms/frame | 4.762 | 3.400 | 2.315 | 2.225 |
| Slowest 1% mean, whole capture, ms | 16.42 | 10.08 | 9.12 | 7.54 |
| End process RSS, MiB | 2657.4 | 1139.4 | 1396.6 | 1154.8 |
| End section arenas, MiB | 1019.0 | 792.0 | 787.3 | 787.3 |
| End live section vertices, MiB | 932.6 | 750.0 | 749.8 | 749.7 |

The DX9 scene had 3,272 visible sections before F3+A. The first post-
reload snapshots at 8.2, 13.2, 18.2 and 23.2 s have 1,554, 2,293,
2,906 and 3,272 visible sections. The rise from 479 to 982 MiB of
buffer bytes tracks that refill. Displayed geometry initially drops
and the frame rate temporarily rises. At 15–20 s, DX9 runs at 117.4
FPS with 1.722 ms of index planning/cold draws, 2.624 ms of solid
and 0.452 ms of arrival uploads per frame. The staged arrival
completion and arena logs show both page fragmentation and subsequent
compactions. The wgpu arena size and used geometry are already close
to the starting values at the 10 s snapshot. The samples bound the
reload progression without timing a complete-image threshold.

Both versions call `all_sections_changed` and rebuild loaded sections.
DX9 converts each CPU section to atlas geometry and retained shadows
on a single staging worker, then allocates immutable native VBs.
The tested wgpu revision uploads packed input to ranges of shared arena
buffers using staged copies, as in `gfx.rs::upload_mesh`. Its
`SectionLists::draw` reuses bindings for adjacent page runs and uses
multi-draw where supported. DX9 still makes about 1,800 solid draws
per frame late in this RTX run. Over seconds 20–60, DX9 spends
2.165 ms on solid, 0.768 ms on translucent and 0.758 ms on solid
plans/cold draws. Its shared `prepare` also takes 1.116 ms/frame
versus 0.230–0.274 ms/frame on wgpu, with most of the difference in
block-entity geometry. These are separate limits on the comparison.
DX9 has no GPU timestamps. Wgpu's health GPU medians in this interval
are about 2.88 ms on GL, 2.26 ms on DX12 and 2.40 ms on Vulkan.

At the last full DX9 log window, about 98.5% of terrain buffer bytes
use the wide 28-byte layout. The 20-byte layout requires exact position,
UV and light coordinates throughout a section, so an unrepresentable
position can put the section in the wide format. DX9 also retains
281.7 MiB of encoded CPU shadows for re-upload,
eviction and Reset. Arena bytes are allocation counts. RSS includes
the whole process and driver mappings and gives no exact VRAM
measurement. Together, the live payload, unused arena space and
CPU shadows establish specific differences. They leave some of the
process RSS gap unattributed. Future GMA decisions use its two scenes
and their geometry and frame costs. The RTX paths are a separate
architecture comparison.

### Write FFP shadow deltas directly during terrain preparation

The next dev candidate E changes one step of CPU mesh preparation in
`crates/horizon-client-d3d9/src/geometry.rs`. D copies interleaved
vertex bytes, computes the bytewise XOR against the previous vertex
and transposes the deltas into planes before LZ4. E writes each delta
directly into the plane using the previous source byte. It reuses the
encoder's existing scratch buffer and carries 32 previous bytes
across each 16-vertex block, including partial blocks. The bytes
passed to LZ4 and the compressed and raw shadow layouts are identical.
The change is active for full FFP encoding in dev builds. The 20/28-byte
hardware formats use the existing encoder.

This removes CPU work from the bounded worker that prepared about
3,800 sections in each long C walk. It also applies when that worker
compresses an edit's previously published raw shadow. The packing
and draw algorithms from D consume the same vertex bytes and material
ranges. The codec change adds no budget, resource ownership or
invalidation dependency.

The host check compares the exact pre-LZ4 bytes to the old algorithm
across block edges and scratch reuse. The 34 host DX9 dev tests and
32 ordinary tests passed. A Xeon Platinum 8573C run timed the real
`MeshData::from_encoded` operation including LZ4 on synthetic section
data. These are medians of nine alternating runs with the pinned
compiler, ThinLTO and SSE3/SSSE3. Each pair produced the same output size.

| Vertices per section | D, µs/section | E, µs/section |
| ---: | ---: | ---: |
| 512 | 28.506 | 17.192 |
| 2,048 | 112.889 | 60.398 |
| 8,192 | 456.366 | 250.632 |
| 32,768 | 1800.695 | 978.223 |

The host result describes encoding cost. Use the existing
`realgmaD_walk.hzperf` and `realgmaD_stand.hzperf` as controls for
300 seconds of walk and 180 seconds of standing on E. Compare
staging completed counts, conversion wall time, frame distributions
and solid work. Confirm the build SHA in the startup log before
recording. The long C duplicates establish a reference for this
route's timing variation.

### Windows 7 packages for direct FFP shadow encoding

[Run 45594475289](https://github.com/Mamsikas/horizon-legacy/actions/runs/45594475289) built E
`f9b94f9f45ce480b258d40f4ed728a1ef423ab35` from `dx9-dev`.
Both packages passed the native import audit, 15 Win7 import-policy
tests and the CPU resource and embedded SM2 tests. Local client
checks passed for dev x64 and x86 and ordinary x64.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| E x64 | [15014131349](https://github.com/Mamsikas/horizon-legacy/actions/runs/45594475289/artifacts/15014131349) | `514e7cbca647295ed8b1f4eb6853772eb4115e55e218ff008c051ef74a29abde` |
| E x86 | [15013409265](https://github.com/Mamsikas/horizon-legacy/actions/runs/45594475289/artifacts/15013409265) | `073fb2c717db45a0242bde93f7846816e6b97f6fa12fc25924b6e74627653775` |

The x64 package matches the GMA control architecture. GMA and HD4570
graphics, resize and Reset checks await native testing of E.

### GMA direct FFP encoding result, 2026-11-25

`E.log` identifies clean E `f9b94f9f45ce480b258d40f4ed728a1ef423ab35`
and matches all three capture starts. The Chell 1.8b GMA3150 HAL, SWVP,
FFP32, RGB565, 1024×600, toolchain and architecture match the verified
long D control. Use `E_walk2.hzperf` for the full route comparison.
Only the first 145 seconds of the other walk are used to compare the
route before the reported zombie interruption.

| Metric | D | E walk2 |
| --- | ---: | ---: |
| Walk seconds / frames | 299.301 / 4399 | 299.129 / 4487 |
| Walk FPS | 14.698 | 15.000 |
| Walk DX9 wall, ms/frame | 50.439 | 49.507 |
| Walk solid wall, ms/frame | 29.268 | 28.641 |
| Walk frame p50 / p95 / p99, ms | 65.43 / 103.88 / 121.70 | 64.99 / 99.77 / 119.52 |
| Walk slowest 1% mean, ms | 170.39 | 146.46 |
| Walk solid indexed draws/frame | 159.68 | 161.81 |
| Walk solid indexed NumVertices/frame | 86,179 | 85,971 |
| Standing FPS | 38.672 | 41.176 |
| Standing frame p50 / p95 / p99 buckets, ms | [20,25) / [30,40) / [40,50) | [20,25) / [30,40) / [30,40) |
| Standing slowest 1% mean, ms | 49.91 | 42.15 |

The 58 whole log windows in each walk contain 3,856 staged section
results on D and 3,859 and 3,846 on E. Total conversion wall is 11.20 s
on D and 8.61 and 8.56 s on E. This corresponds to 2.904 ms/section
versus 2.230 and 2.225. Even over just the common first 145 seconds,
the means are 2.872 ms on D and 2.244 and 2.254 on E. This measures
the whole parallel conversion stage on GMA, including scene-dependent
work. The exact byte checks and the host codec experiment isolate
the reduced encoding work. Keep E in dev.

The same first 145 seconds reach 14.490 FPS on D and 14.476 and
15.028 on E. On the common first 295 seconds, clean E gains 0.308 FPS
and 0.660 ms/frame in the sum of solid, cache and indices/cold draws.
The terrain gain persists when any one 15-second segment is omitted.
Clean E has 38 frames over 125 ms versus 34 on D, with smaller worst
spikes overall. Its most expensive staged section took 110.73 ms
versus 24.03 on D. Neither standing run performed bulk staging or
resident shadow decode during the recorded windows. The standing
FPS difference therefore cannot measure encoding cost directly.
Native windows have zero COM failures or aborted frames. DX9 GPU
timestamps remain unavailable.

### Reuse texture-stage and sampler values after a successful Present

The E log has an average of 39.6 actual sampler setters and 77.0
texture-stage setters per walk frame. Standing has 13 and 19. The
solid pass accounts for 12 and 16 per frame in either scene. E
forgets these values at each frame start, even when the last requests
from the preceding scene agree with the first requested values.
Reuse the native device's tracked numeric values through a clean
Present on the dev SWVP path.

`device.rs` moves the existing sampler and stage slots onto the
native device owner so a renderer reuse shares the same record of
actual COM values. The setter wrappers keep the existing requests
and actual-call counters. `texture_state_cache.rs` remembers only
successful setters. A failed setter forgets its slot and an incomplete
frame resets the table on the next begin. Lost device, Reset,
presentation rebuild and resource-pack renderer replacement invalidate
the table. D3D9
render state, resource bindings, shader state and transforms still
follow their frame-start setup. Hardware VP and ordinary builds
use a fresh table at frame start.

The table has 141 numeric entries and one frame flag per device. It
holds no COM resources. The CPU test checks repeat elision, a failed
setter, a frame without Present, explicit invalidation and the
per-frame fallback. All 35 dev tests and 33 ordinary tests passed
on the host. The hardware comparison uses the existing clean
E walk and standing as the control. Compare actual sampler and TSS
sets per frame alongside draw and vertex counts, pass wall time and
frame distribution. Also exercise fog, texture filtering, HUD clips,
glint, resize and resource-pack reload on the native device.

### Windows 7 packages for texture-state reuse

[Run 45603122656](https://github.com/Mamsikas/horizon-legacy/actions/runs/45603122656) built F
at `4e9eddda3b61b92f6f886c4990227a3184d24f0f` on `dx9-dev`.
Both architectures passed the Win7 executable import audit, 15 import-policy
tests, and the CPU resource and embedded SM2 tests. Local client checks
passed for dev x64 and x86 and ordinary x64. A host replay of the actual
cache logic matched the baseline numeric state after 983,040 synthetic
requests across 4,096 frames, including simulated setter failures and
invalidations. Real driver rendering and time remain hardware checks.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| F x64 | [15017957505](https://github.com/Mamsikas/horizon-legacy/actions/runs/45603122656/artifacts/15017957505) | `808b54a520d0443987fda8c238bbabbb0a43d1ec3eca81aef35174bc6b20d1d2` |
| F x86 | [15017762116](https://github.com/Mamsikas/horizon-legacy/actions/runs/45603122656/artifacts/15017762116) | `d74865c14d48b39c6bbb78b71e26867c3ba7c7df766c2c922b4203c56debbe26` |

Use `E_walk2.hzperf` and `E_stand.hzperf` as the controls for
F x64. Capture 300 seconds of the same walk, 180 seconds standing,
and the log. The F startup line reads
`D3D9 texture_state_cache retain_after_present=true` on dev SWVP.

### GMA texture-state reuse result and rollback, 2026-11-25

The F log identifies the clean F SHA above and matches both capture
starts. E and F use GMA3150 Chell 1.8b, HAL SWVP, FFP32, Win7 x64,
RGB565 and 1024×600. The captures expose the same recorded settings.
E walk2 is the full route control.

| GMA metric | E | F |
| --- | ---: | ---: |
| Walk seconds / frames | 299.129 / 4487 | 298.944 / 4347 |
| Walk FPS | 15.000 | 14.541 |
| Walk DX9 wall, ms/frame | 49.507 | 50.953 |
| Walk solid wall, ms/frame | 28.641 | 29.327 |
| Walk entities / translucent / GUI wall, ms/frame | 8.907 / 3.235 / 4.684 | 9.265 / 3.358 / 4.813 |
| Walk frame p50 / p95 / p99, ms | 64.99 / 99.77 / 119.52 | 66.31 / 104.09 / 126.51 |
| Walk slowest 1% mean, ms | 146.46 | 170.05 |
| Walk sampler / TSS calls per frame | 39.60 / 76.96 | 28.68 / 58.20 |
| Walk solid sampler / TSS calls per frame | 12 / 16 | 1 / 1 |
| Walk solid indexed draws / NumVertices per frame | 161.81 / 85,971 | 163.41 / 86,254 |
| Standing seconds / frames | 180.641 / 7438 | 179.864 / 7076 |
| Standing FPS | 41.176 | 39.341 |
| Standing DX9 / solid / translucent wall, ms/frame | 20.001 / 8.977 / 7.324 | 20.963 / 9.382 / 7.694 |
| Standing frame p50 / p95 / p99 buckets, ms | [20,25) / [30,40) / [30,40) | [20,25) / [30,40) / [40,50) |
| Standing slowest 1% mean, ms | 42.15 | 51.35 |
| Standing sampler / TSS calls per frame | 13 / 19 | 2 / 4 |

There were 58 full log windows per walk (4,387 E and 4,255 F frames)
and 35 E and 34 F standing windows. No COM failures or aborted
frames were recorded. The setter reduction is reproducible in the
log. The observed frame and DX9 wall times worsened in both scenes.
Standing advertised 24,624 solid and 20,084 translucent vertices in
both tests, with 29 versus 31 solid draws and 1.00 versus 3.13 GUI
DrawPrimitive calls. Its median complete-window FPS was 44.90 versus
44.06. The larger mean difference includes several slower windows.

The first 30 seconds of walk show 13.53 versus 12.27 FPS and 52.25
versus 57.89 ms DX9 wall. The 145–295 second segment shows 14.99
versus 14.85 FPS and 49.57 versus 49.88 ms, with p99 improving from
114.65 to 112.18 ms over that segment. On the common first 295 seconds,
omitting any one 15-second segment still leaves F down by 0.31–0.50 FPS.
The earlier two E walks differ by 0.55 FPS over their common first
145 seconds before the reported interruption. Time-aligned five-second
windows rarely matched both solid ranges and entity geometry closely.
Renderer wall includes driver work and possible waits. The aggregate
counters and sampled thread time cannot price the skipped setters or
assign the slowdown to the cache itself. F has no measured benefit
that offsets its slower recordings, so restore E's per-frame sampler
and texture-stage setup for the next trial.

### Pack compatible solid ranges on first upload

The GMA F walk moves through 1,379 new arrival pages covering 2,336
section arrivals between the first and last complete log windows.
The median snapshot has 10 material-packed arenas out of 24 resident
arenas. Packing currently occurs during resident compaction. On the
newly loaded pages, solid still submits separate ranges around other
materials and translucent spans until a replacement arena is built.
The captured walk spends 29.33 ms/frame on solid with 163.41 indexed
draws and 86,254 vertices in the submitted ranges. The sky span is only
0.04 ms/frame with the tested options, so upstream's reverse-Z sky
reordering in `e549bffef3` is outside this experiment.

Horizon wgpu's `gfx.rs::upload_mesh` places new sections in shared
pages, and `SectionLists::draw` reuses vertex and index bindings across
adjacent page runs. The DX9 arrival path already batches compatible
sections into one native VB. G applies the existing resident material
packer at this first upload in a dev SWVP build. Moving the same
material ranges together allows the existing solid draw plan to
merge adjacent visible ranges. Each draw retains its exact 16-bit
indexed bounds. The driver still processes the same submitted
vertices for identical visibility. There is no new native buffer
allocation beyond the arrival VB.

`legacy/mesh/materials.rs` is shared by resident compaction and
`legacy/mesh/arrival.rs`. An arrival page must contain at least two
whole current sections with contiguous source coverage. The packer
retains original primitive order within each material and retains
source order within each section's translucent ranges. Existing fog,
cutout, facing and visibility rules still decide which ranges can
merge. The former layout serves single or split sections, holes from
stale revisions, or pages outside the 512-range budget or without more
material adjacency. Synchronous edits, demand loading, ordinary
builds and HW VS2 use their previous arrival path. The original
CPU shadow and existing Reset/eviction lifecycle are retained.
Packing adds at most one temporary encoded page below 2 MiB plus
bounded metadata. It adds a render-thread copy only when packing
succeeds. A failed native upload still falls back to demand loading.

`mesh.rs` selects the bulk arrival path. `legacy.rs` and
`legacy/frame.rs` expose `material_packed_pages_total` on the
existing diagnostic line. `d3d9_arrival_material_pack` measures
planning and copying within the enclosing arrival span. For the
same GMA route, compare that arrival cost, solid counts, ranges'
NumVertices, solid wall time and frame tails to E walk2. Standing
is a regression check. The CPU cases check byte-identical source
slices, stable material order, and original-layout fallbacks.

### Windows 7 packages for first-upload material packing

[Run 45619586052](https://github.com/Mamsikas/horizon-legacy/actions/runs/45619586052) built G
at `9d042bbfa4efd8c26c136bc36797cb7712319c27` on `dx9-dev`.
Both packages passed the executable import audit, 15 import-policy
tests per architecture, and the CPU resource and embedded SM2 checks.
Local checks covered dev x64 and x86 and ordinary x64. The host passed
34 dev and 32 ordinary DX9 tests. Four targeted material tests covered
exact byte mapping, fallback for incomplete or noncontiguous arrivals
and draw order under different visibility and fog masks.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| G x64 | [15026686277](https://github.com/Mamsikas/horizon-legacy/actions/runs/45619586052/artifacts/15026686277) | `67e09be5ac25f12488b08995b624e59d07ea49b7d70fe9174fadb9023712d807` |
| G x86 | [15027301332](https://github.com/Mamsikas/horizon-legacy/actions/runs/45619586052/artifacts/15027301332) | `5c0ebc9c6fbaef9467f1526a92a9c58fc30c57b5d2c47f6090f52d25b8e9ddc7` |

Use G x64 with the existing E x64 control recordings. Record the
same 300-second walk and 180-second standing, plus `latest.log`.
The log identifies G by `source_sha` and reports
`material_packed_pages_total`. Compare solid and arrival wall time
together with draw counts, range vertices and frame tails.
Native GMA3150 and HD4570 rendering, resize and Reset remain to be
checked. The package checks establish Win7 imports and build coverage.

### GMA first-upload material packing result and rollback, 2026-11-25

`G.log` identifies the clean G x64 SHA above, GMA3150 Chell 1.8b,
HAL SWVP FFP32, 1024×600 and RGB565. Its profiler starts match
`G_stand.hzperf` and `G_walk.hzperf`. Measurements use the recorded
intervals, after the owner waited for the server ping to settle.
The recorded settings match E.

| GMA metric | E | G |
| --- | ---: | ---: |
| Walk seconds / frames | 299.129 / 4487 | 300.017 / 4489 |
| Walk FPS | 15.000 | 14.962 |
| Walk DX9 wall, ms/frame | 49.507 | 49.580 |
| Walk solid wall, ms/frame | 28.641 | 28.437 |
| Walk arrival uploads, ms/frame | 0.287 | 0.351 |
| Walk first-upload material packing, ms/frame | — | 0.062 |
| Walk frame p50 / p95 / p99, ms | 64.99 / 99.77 / 119.52 | 64.46 / 100.89 / 122.32 |
| Walk slowest 1% mean, ms | 146.46 | 155.98 |
| Walk solid indexed draws/frame | 161.81 | 146.45 |
| Walk solid indexed NumVertices/frame | 85,971 | 86,722 |
| Standing seconds / frames | 180.641 / 7438 | 183.361 / 7302 |
| Standing FPS | 41.176 | 39.823 |
| Standing DX9 / solid / translucent wall, ms/frame | 20.001 / 8.977 / 7.324 | 20.674 / 9.314 / 7.578 |
| Standing solid indexed draws / NumVertices per frame | 29 / 24,624 | 30 / 24,624 |
| Standing frame p50 / p95 / p99 buckets, ms | [20,25) / [30,40) / [30,40) | [20,25) / [30,40) / [30,40) |
| Standing slowest 1% mean, ms | 42.15 | 50.46 |

G's first and last complete walk windows track 1,360 new pages and
571 material-packed uploads. The median resident arena count is 22
in both walks, with 10 packed in E and 16 in G. Counts use 58 complete
E and 59 G windows. These show 9.5% fewer solid draw calls with 0.9%
more advertised vertices. Measured render-thread busy time is 93.7%
and 93.9% of the respective walk intervals. There were zero aborted
frames and no recorded COM failures. Hardware GPU timestamps are
unavailable on these recordings.

The first 30 seconds gain 0.67 FPS and 1.97 ms/frame of solid wall.
Over seconds 30–295 the solid saving is 0.062 ms/frame, alongside
0.061 ms/frame more arrival time. Over the common first 295 seconds,
omitting any one 15-second segment leaves an FPS change of
−0.16 to +0.05. G's standing has no arrivals or compaction during
the capture. It makes one more solid draw per frame, and there is
frame-to-frame timing variation in other passes. These results
cannot establish a packing-related standing slowdown.

Restore E's bulk upload layout. The draw count fell, while the
timings show no sustained renderer or frame-time benefit. Resident
material compaction remains the measured baseline.

### Reuse the DX9 atlas dimensions in UV conversion

On GMA G's walk spends 4.40 ms/frame in
`d3d9_dynamic_entity_convert` and 3.64 ms/frame in dynamic submit.
The native dynamic pass advertises 8,573 indexed vertices and
5.89 draws per frame. Recent terrain experiments reduced draw
counts while overall frame time stayed close to the control.
The next trial targets arithmetic in the existing DX9 atlas mapper.
Horizon wgpu's `entity.wgsl` can use base atlas UVs directly on its
ordinary entity path. DX9 must map from the shared virtual image to
resident physical pages. Its sprite path already stores a page
reciprocal in `AtlasUv`.

In `legacy/atlas.rs`, dev `PixelView::polygon` uses the existing
tile grid dimensions and shifts by the base-two exponent of its
tile size. Integer truncation and clamping remain exact. The
final normalization of physical coordinates multiplies by a
reciprocal computed once per `PixelPages` layout. Each physical
side is a power of two. Multiplying by this exact reciprocal
preserves the bits of the prior division for finite coordinates.
Virtual image dimensions can be arbitrary. The clipped polygon
bounds still use their original calculations.

The patch adds one f32 per physical layout, with the value rebuilt
when the layout is created. The main-atlas fallback uses the main
page scale. Test fixtures exercise fractional texels in a 17×13
virtual image, exclusive upper tile boundaries, clipping seams,
placement on both layouts, and fallback when compact slots are full.
The experiment is selected by the existing dev build. The code
uses the same atlas geometry on FFP and hardware VS2.
Compare the walk's dynamic convert span to E walk2 at comparable
submitted vertices and compare overall frame-time tails. Standing
also exercises the shared atlas path through the HUD.

### Windows 7 packages for atlas UV conversion

[Run 45645322017](https://github.com/Mamsikas/horizon-legacy/actions/runs/45645322017) built H
at `7d14444711e3625997c103c935cd4fbde9a15462` on `dx9-dev`. Both builds
passed the Win7 import audits, 15 import-policy tests per architecture,
and the DX9 CPU resource and embedded SM2 checks. Local Windows client
checks passed on dev x64 and x86 and ordinary x64. The host passed
34 dev and 32 ordinary backend tests. Three targeted atlas tests passed
against both conversion paths.

A host replay used the two live mapping routines on 294,912 quads
per timed sample, with ordinary and clipped UVs on each layout.
Each pair emitted matching page and UV hashes. The hot mapper's
x64 assembly uses two integer division sites and one packed float
division site, compared with eight and four on E. The host median
times are 22.03 to 21.80 ms on the shared pages and 22.32 to 22.76 ms
on the compact page. These changes are within the sample spread.
Native GMA CPU time, frame pacing and graphics remain hardware checks.

| Package | Artifact | GitHub ZIP SHA256 |
| --- | --- | --- |
| H x64 | [15037214435](https://github.com/Mamsikas/horizon-legacy/actions/runs/45645322017/artifacts/15037214435) | `0286eb828fec3cdf5bf2ad971a18d47855e01a5382671b80e06ab12ed25b1792` |
| H x86 | [15040415355](https://github.com/Mamsikas/horizon-legacy/actions/runs/45645322017/artifacts/15040415355) | `1e2997faa714f1cda5ad2c1b5d74d5fce99600d07d45013f0cb9145f729f4765` |

Record the same 300-second walk and 180-second standing on H x64
with `latest.log`. Compare to the existing `E_walk2.hzperf` and
`E_stand.hzperf`. The main comparison is dynamic entity conversion
at comparable submitted geometry, with the GUI and whole-frame
times as regression checks. Native checks on GMA3150 and HD4570
are pending.
