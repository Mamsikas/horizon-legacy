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
