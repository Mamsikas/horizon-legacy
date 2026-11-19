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

Hardware counters and diagnostic overhead are pending, so P0 is not accepted.
Per-material/page/overlay/atlas-clipping counters, entity convert-vs-submit and
GUI patch/cache attribution, solid density/reuse distributions, asynchronous
GPU queries and synthetic controlled driver tests are still open. This patch
does not infer their values from wall times. First use the split pass table and
NumVertices/draw volumes to choose the next small measurement or gated SWVP
dense-window experiment. No geometry/state optimization has been accepted yet.
