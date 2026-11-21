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
