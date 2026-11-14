# Windows 7 Direct3D9 client preview

Current upstream integration: `298c01b0206d72294b25753c9f8d0a77b742b3bb` (this fork's main,
13 November 2026). Merged into the existing development branch without rewriting its history.
Scope: Windows 7 SP1 **x64**, Atom N455/GMA3150 first, HD4570 second. No x86 deliverable.
The ZIP's executable selects DX9 by its `legacy-d3d9` feature, without changing other backend implementations or any saved graphics defaults.
At the owner's request, its default initial window is 854x480. Explicit launch
dimensions/fullscreen still take precedence; saved graphics values are unchanged.

The owner has now run the game on N455/GMA3150/Win7 x64: about 50 FPS in a flat
world and 30 in a normal world at two chunks, with roughly 500–750 MiB reported
process memory. This is an owner hardware result for the preceding build, not a
measurement of this update. HD4570 gameplay remains unverified.

## Dependency and integration review (R0)

| Source | Finding / reuse |
| --- | --- |
| `horizon-client/{main,entry,lib}.rs`, `game.rs` | Keep startup, authorization, resources, game state and event processing. Change the gameplay renderer type import only. |
| `game/{event_loop,window_handler,startup_menu,render_tick}.rs` | Keep winit 0.30.13, HWND, keyboard/mouse, IME, frame generation and ordinary FPS limiting. DX9 receives the same CPU `Frame`. |
| `game/loading_window.rs` | Current HEAD moved the constructors here. A build-time adapter supplies both loading/title and game renderers; it avoids modern mip/PBR preparation and direct wgpu device access. |
| `game/{world_update,world_loading,packs,frames,postprocess}.rs` | Upload changed sections and release columns through the adapter. Keep resource reload, dimension layout, block entities and all gameplay. Suppress only legacy LOD workers. |
| `horizon-client-render/gfx.rs`, `gfx/{resident,arena}.rs` | Modern Renderer is GPU-specific, with public config/HUD/stats fields and large arenas. Reuse Frame, Frustum, View and metadata types, never its device or arenas. Two additional helpers expose existing CPU frustum/lightning generation. |
| `streaming.rs`, `mesher{,/packed,/lighting}.rs`, `chunk.rs` | Existing workers deliver SectionMesh. Full solid/translucent quads remain available, with 24-byte TerrainVertex, UV accessor, AO/tint/light, bounds, Sides, visibility and block entities. Legacy disables packed layout; meshing stays unchanged. |
| `model_library.rs`, `models/`, `block_entity_render.rs` | Keep non-cube, item/entity and block-entity CPU geometry. Advertise no GPU skinning so ordinary entities are posed on the existing CPU path. |
| `sprites.rs`, `assets.rs`, `atlas_sources.rs` | Convert existing RGBA layers into padded, caps/budget-sized POT Texture2D pages. Keep source layer numbers; remap only legacy vertices. Extrude each sprite's mip levels independently. Animate changed frames only. |
| `hud.rs`, `hud_layout.rs`, `font.rs`, `screen_effects.rs` | At this HEAD HUD data is **triangles**, and adds Picture/DepthGlint runs. Reuse triangles, alpha/depth/scissor, glyph/map/skin patches and standalone pictures. Split the existing atlas into lazy pages; never rebuild the GUI. |
| `camera`, `light`, `fog`, `weather`, `particles`, `clouds` | Keep camera math, CPU block light, particles and weather inputs. Fixed-function stage 0 multiplies the sprite and tint/AO; stage 1 samples a 16x16 day/night lightmap. Meshes do not upload when the sun moves. Embedded PS2 fog keeps the spherical environmental and cylindrical distance curves; fixed-function fog remains a caps fallback. |
| `graphics.rs`, `options_ui.rs` | No defaults or serialization changes. Settings remain stored even when this renderer does not implement an effect. `picked_pack` returns an empty effective pick only in the legacy build, preventing pack failure code from clearing the saved pick. |
| Cargo, `horizon-client-input`, `horizon-threads`, workflows | Keep unsafe forbidden in existing client crates. COM/locked memory belongs to the new !Send/!Sync D3D9 crate. Package audio via existing WASAPI; omit controllers/WinRT and Horizon extras in this legacy executable. Keep ordinary default features. |
| `horizon-prepare`, `docs/native-client.md`, Win7 launcher | Reuse the prepared 26.3 `--resources` contract. The game does not run Java for prepared resources. First installation/exporting belongs to the existing launcher/preparer; no JDK25-on-Win7 assumption is added. |

`renderer::Renderer` is a build-time safe adapter rather than an enum that allocates
both GPU engines. Its lifecycle covers construction, section upload/removal, dimension
clear, resource-pack replacement, render, resize and shutdown. Compatibility methods
report unavailable GPU effects explicitly. `gfx::Frame` is shared; no second world,
input mapping, mesher, inventory or resource system was introduced.

## Hardware boundary and lifecycle

Only `Direct3DCreate9` / `IDirect3D9::CreateDevice(D3DDEVTYPE_HAL)` is used. Require
HWRASTERIZATION, two texture stages, MODULATE, alpha test/blend, supported RGBA
textures and a matching D24X8/D16 depth format, then test real texture/VB allocation.
Use hardware fixed-function vertices when HWTRANSFORMANDLIGHT is exposed, otherwise
software vertex processing on HAL. No hardware VS2 requirement and no REF/WARP path.
GMA uses fixed-function world vertices and CPU-projected XYZRHW GUI vertices.
Capable hardware may use the existing embedded VS2 compact terrain path. An
embedded `ps_2_0` program combines the fog curves when PS2 and texture-coordinate
generation are available; otherwise a fixed-function approximation is logged.
No shader compiler or SDK runtime is required.
Intel's [N400/N500 datasheet, section 3.2.1](https://www.intel.com/content/dam/www/public/us/en/documents/datasheets/atom-n400-vol-1-datasheet-.pdf)
assigns transform/lighting to the CPU, with setup, texturing and rasterization on
the GPU (two 200 MHz pixel pipelines). Its CPU feature list confirms Intel64,
SSE2, SSE3 and SSSE3. This is why HAL software vertex processing is the expected
GMA3150 baseline; a caps-exposed hardware mode still needs performance measurement.

Intel's [technical description](https://community.intel.com/t/atom-nm10-d510m0-graphic-details/59870/1)
explicitly identifies hardware PS2.0. Its [summary table](https://www.intel.com/content/www/us/en/developer/articles/guide/hd-graphics-quick-reference-guide.html)
labels GMA3150 SM3.0; this does not prove hardware VS3/PS3. Device caps, HAL creation
and physical measurements decide the path. Microsoft documents
[HAL with software vertices](https://learn.microsoft.com/en-us/windows/win32/direct3d9/device-types)
as hardware rasterization. No GPU vendor IDs decide compatibility.

A small implicit swapchain anchors the device. A window-size additional swapchain and explicit depth surface handle resize/vsync changes without Reset or terrain loss. Screenshots read the active render target. Managed textures/IBs survive a real Reset. On NOTRESET, additional surfaces and DEFAULT resources/bindings are released on the creating thread, then reconstructed from CPU shadows. LOST waits for cooperative recovery. Minimized windows skip drawing. Resources retain their
device/window through owned COM handles and cannot move to another thread. Reverse Z
uses a real depth surface; paired view/projection Z reflections keep the common clip
coordinates and positive fixed-function fog depth.

Each VB is bounded by 16-bit indices and caps (at most 65,532 vertices); no 256 MiB
arena pages. Section meshes convert once into lossless delta/byte-plane LZ4 CPU shadows (including exact floating-point bits).
The cache decodes only to load missing GPU data or draw uncached geometry. Nearby
sections share immutable VBs with positions local to a 128-block region;
negative coordinates use Euclidean division and the dimension's minimum Y is kept.
New pages upload once and are never modified while draws can reference them. This
also avoids placing permanent terrain in dynamic/AGP storage on discrete GPUs.
FFP software-VP input uses SYSTEMMEM; hardware-VP input uses DEFAULT. Microsoft
documents [CPU-readable input for software vertex processing](https://learn.microsoft.com/en-us/windows/win32/direct3d9/device-types-and-vertex-processing-requirements).
The memory pool changes input placement, not HAL rasterization.

On hardware vertex processing, visible solid quad ranges from multiple sections
become one indexed batch per arena/page/alpha mode. The existing CPU column/frustum walk, vanilla portal visibility graph and Sides culling select geometry; spectator bypass is preserved. Visibility index buffers are retained until their canonical quad ranges change, with one bounded index scratch instead of permanent per-batch CPU index arrays. On software vertex processing, narrow direct draws
avoid transforming holes between sparse index ranges; its visibility plan is
cached too. Translucent sections draw far-to-near. Within each section, vertices
are packed by texture page at upload; the few page ranges are ordered by bounds
when the camera crosses a block boundary. Each resident draw uses a tight
contiguous range and the shared quad IB, including on software VP. Alpha blends
keep depth testing and back-face culling, but do not write depth between the
approximately ordered batches. This is basic transparency, not Improved
Transparency or exact interleaved-quad ordering. Camera motion never reuploads
terrain vertices or allocates translucent index buffers.
Dynamic index and transient vertex rings use
NOOVERWRITE on append and DISCARD only on wrap, rather than renaming a full buffer
for every small draw. Terrain GPU residency is bounded, with offscreen pages evicted
first; an allocation failure still permits transient draws. Missing visible geometry is cached on its first draw instead of repeatedly streaming it for seconds behind a two-millisecond warmup cap. One CPU compaction worker consolidates small VBs into at most one 2 MiB page per job. Jobs/results are bounded, carry immutable shadows and revisions, and keep old GPU data until a successful replacement allocation. This avoids render-thread decompression for compaction. The automatic target is
one third of the driver's available-memory hint, bounded by one eighth of system RAM
and 16 MiB–2 GiB. It no longer imposes the GMA-scale 128 MiB ceiling on an RTX.
Render distance and texture quality are never rewritten. CPU shadows, physical
arena allocations, live GPU vertices and decode/index scratch are reported separately.

Sprite pages group native resolutions, undoing only array upscaling (a 16 px sprite
in a 64 px CPU array keeps its original detail). Animated sprites have separate
pages at most 256 px, except when the sprite itself needs more space. Static pages
are at most 1024 px, subject to caps. GUI keeps the existing virtual atlas and packs used 128 px tiles, with neighbouring
texel gutters, into physical pages of 1024 or 2048 px, subject to caps and the
texture target. Access order keeps active fonts/fills/icons together. Unused tiles
allocate no GPU storage; immutable CPU tiles compress and patched tiles stay raw.
Glyph/map/skin patches update tile seams before resident cells are uploaded. Per-sprite
extruded mip chains remain. Panorama uses the same dedicated camera matrix as the
ordinary renderer and releases storage on entering a world. Advisory memory targets
derive from the driver's available-memory hint. Exceeding the texture target warns;
exceeding the GPU mesh target falls back to transient draws, not a fatal budget
error. Optional environment overrides remain for diagnosis; ordinary use needs none.

The first PE inspection found modern GPU dependencies despite never creating a wgpu
device. Cargo now omits Vulkan/Metal/DX12/GLES **only for the Win7 vendor target**;
ordinary platform feature sets are unchanged. `horizon-hdr` has a Win7 no-Vulkan
adapter and the build enables `windows_slim_errors`, avoiding optional WinRT rich
error origination. A second finding was arboard's static Win8 `PathCchStripPrefix`
import. The Win7 input layer selects a small Unicode clipboard shim in the isolated
presentation crate; normal platforms still use arboard. Keyboard/mouse/IME mappings
and event handling are unchanged. The PE audit requires Direct3DCreate9 and rejects
other graphics/shader DLLs in both normal and delay imports; unknown API contracts
remain errors, not accepted wildcards.

## Preview features

| Feature | Legacy path |
| --- | --- |
| Solid/cutout/translucent terrain, non-cube models, fluid surfaces | Existing SectionMesh; paged textures, alpha/Z, section transparency sorting |
| AO, biome tint, sky/block light, day/night, night vision/darkness | Existing vertex data and 16x16 lightmap; no all-section recoloring |
| Title, menus, inventory, chat, glyph/map/skin updates, cursor | Existing HudBatch and input; GUI depth/scissor/blends |
| Entities, block entities, hand/held items, world text, particles | Existing CPU vertices; basic normal shading and material blending |
| Rain/snow, lightning, breaking cracks, block outline, border, screen overlays | CPU inputs and fixed-function geometry |
| Nether/End, panorama, sun/moon | Octagonal sky-disc horizon gradient and dual PS2 fog; simplified textured End sky and portal |
| Animated textures/filtering | Shared native-size/linear-light/alpha-coverage mip generation; nearest with Texture Filtering=None, linear/trilinear for nonzero filtering when caps allow. RGSS/anisotropy are approximated by linear filtering; exact mip-count clamp, animation interpolation/Polytone expressions remain unavailable |
| Fancy clouds/star field, foil/glint, glowing outlines, pack core shaders | Omitted in this preview; base objects/items remain |
| LOD, GPU occlusion/culling/skinning, shader packs, PBR, HDR, post chains, TAA/FXAA, frame generation | Unavailable; no corresponding GPU pipelines start; settings remain stored |
| Split-screen compositor / GPU render capture | Explicitly unavailable; screenshots and DX9 log counters remain |

Static block-entity snapshots currently use the dynamic entity draw path. GPU caching
of those optional snapshots and CPU-transformed vertex
A/B testing remain performance follow-ups requiring hardware evidence.

## Current optimization and profiling pass

Owner measurements of build `7d536626fb`: about 950 FPS / 1.1 GiB at 16 chunks,
135 FPS / about 4 GiB at 48 once settled, with loading dropping toward 50 FPS.
The attached 48-chunk memory snapshot reports 3613.2 MiB resident, 1364.8 MiB
requested terrain storage and 559.4 MiB CPU shadows. These counters are not
physical VRAM. Most hardware vertices remain Wide28, not Compact20.

Build `7ce2bb42c8` regressed transparency badly. The owner's GMA comparison
dropped from 23 to 5 FPS. Its 18.2-second capture averages 6.6 FPS with 124.5 ms
in translucent submission, 9.5 ms solid and 3.1 ms GUI. The static scene has only
20 visible sections but about 3790 draws and 1994 texture binds per frame, with
zero terrain index uploads. Per-quad sorting alternated texture pages; bounding
software-VP index spans split those runs still further. Cached indices avoided
uploads but did not make thousands of driver calls cheap.

The latest 68.8-second RTX loading capture averages 58.3 FPS, with renderer
14.0 ms, solid 6.7, translucent 2.5, plans/cold draws 2.4 and synchronous uploads
0.1 ms. The settled capture averages 105.1 FPS, renderer 7.8 ms, translucent 2.6
and solid 2.3 ms. These are measurements of the regressed build, not this fix.
The log shows over 8300 small terrain buffers decreasing toward 1700 after
loading; merging only when the upload quota was completely empty delayed that
work. Periodic memory/control reports add roughly 100 ms pauses of their own.

This update targets costs relevant to low render distances too:

- A full staging quota now applies backpressure to the existing nearest-first
  game upload backlog. Bulk jobs no longer spill into synchronous conversion
  just because the conservative 8 MiB queue charge is temporarily full. Existing
  and pending section edits go first and remain synchronous. Unsupported/oversized
  jobs retain a synchronous path so a job that cannot fit never blocks the queue.
  Mesher scheduling, graphics defaults and the user's upload limits are unchanged.
- FFP shares the exact solid visibility-plan cache and texture/state grouping.
  Adjacent ranges merge; software VP still transforms only tight contiguous
  ranges rather than sparse whole-page spans. Its immutable input VBs use
  SYSTEMMEM; hardware-VP buffers still use DEFAULT.
- Basic translucent batches now use one contiguous range per texture page,
  split only at native VB boundaries. The CPU check packs 4096 alternating
  quads into three ranges, or 19 draws with deliberately tiny 1024-vertex VBs,
  without changing vertex attributes or winding. Back-face culling addresses
  double-sided ice. Exact overlapping-glass/water order is not promised; the
  owner explicitly prefers basic transparency and speed. No per-quad centers,
  sorting state, cached translucent IBs or scratch buffers remain.
- The same bounded worker can merge resident VBs during bulk loading rather
  than waiting for an empty upload quota. Only one merge is outstanding, capped
  at 2 MiB; speculative nonresident prefetch waits for bulk work to finish.
  Old buffers stay drawable until the replacement succeeds. This reduces a
  source of post-load warmup; it does not change world generation or meshing.
- Alpha GUI and depth/scissor inventory models reuse byte-exact prepared ranges;
  the first model clears depth, then overlapping model pixel bounds or a bounded
  coverage-window reset require another clear. Disjoint slots share depth and
  compatible adjacent draws; necessary clears remain explicit barriers. Only newly present sparse atlas tiles
  invalidate geometry. Pixel-only animation/glyph/map/skin patches keep their UV
  mappings, avoiding global cache rebuilds each animation tick. Pictures, foil
  and invert runs keep ordinary preparation; the existing 2 MiB/128-slot bound stays.
- Software-VP HALs use a separate 28-byte XYZRHW GUI stream, preserving logical
  scale, half-pixel placement, model depth and scissor while bypassing runtime
  matrix T&L. Hardware-VP GUI is unchanged. Failure to allocate the optional stream
  keeps the ordinary FFP path. F3 no longer prefixes DX9 with the metadata's Noop.

The separately requested offline fix completes encryption without a fabricated
session-service join when the profile has no token, preserving the server's
admission/refusal. Previously the client rejected it before the encryption reply.
Signed-in authentication is unchanged. Retest against the owner's offline server
is still needed; no server gameplay result is claimed here.

Existing shared Resident/column/frustum/portal culling, model data, mip generation,
exact solid-plan reuse, 20/28/32-byte hardware/FFP layouts and 4 MiB animation-pixel
cache remain. GMA software vertex processing uses Full32 and does not require a
hardware vertex shader. It benefits from pooled arrivals and GUI reuse too.
The shared wgpu compute/indirect/shader-expanded quad implementation cannot be
transplanted directly to baseline SM2; this path uses compatible CPU mechanisms.

The opt-in .hzperf timeline now also splits atlas conversion, vertex packing,
shadow compression, arrival uploads, GUI preparation, textures, upload and submit.
Windows frame/timeline/memory capture is available; stack/heap sampling remains
Linux-only and DX9 GPU timestamps are explicitly unavailable. Nothing is uploaded
automatically. Diagnostic memory captures have measurable cost; start with ordinary
logs/F3 for plain FPS comparisons. `/profiler start client` and `/profiler stop client`
work through chat without --control or a launcher change. Do not request GPU timing
on DX9. Detailed timeline stops after 60 seconds; take separate loading segments.
Common pre-device resource preparation is outside this work. The new log separates
basic-transparency draws, texture binds and page ranges. No FPS improvement for
this fix, OpenGL parity or physical HD4570 result is claimed before the owner's
hardware retest.

## Build, package and test

`tools/build_win7_client.sh` uses nightly-2026-11-11 with rust-src and
`x86_64-win7-windows-gnu -Z build-std=std,panic_abort` ([Rust target documentation](https://doc.rust-lang.org/rustc/platform-support/win7-windows-gnu.html)).
The pinned MinGW GCC14.2/binutils2.44 cross tools are shared with the existing launcher.
The separate optimized `legacy` profile bounds linker memory; modern profiles stay intact.
Rust flags permit SSE3/SSSE3, never host-native SSE4/POPCNT/AVX. CRT/native support is
linked statically; no redistributable DLL is assumed without the PE audit.

```sh
tools/bootstrap_win7_cross.sh "$PWD/work/win7-client/cross"
# Install rustup locally under work first; the script prints the required locations.
tools/build_win7_client.sh
```

The Actions artifact ZIP directly contains `horizon-client.exe`, `horizon-ctl.exe`,
`TESTING.txt`, checksums, import reports and exact build information; no inner ZIP. Select the client exe in the existing Win7 launcher's native-client
setting. Direct launch: `horizon-client.exe --resources C:\path\to\26.3 --game-dir C:\path\to\test-game`.
Resources and saves are never distributed in this archive.

This iteration has twenty-one CPU tests (including bounded texture-page ranges,
projected GUI coordinates, depth-clear reuse equivalence, ordered index windows,
sparse-tile invalidation, tile seams/placement, executable SM2 arithmetic,
all vertex layouts/partial codec blocks, animation-cache eviction and shared-page source indices) and eleven PE policy/parser tests. The packaging workflow builds
the client/control ZIP and audits static/delay imports,
architecture and subsystem version. Compiler warnings inherited from the current
nightly/native dependencies are separate from a successful check. Full parity harnesses
and a resource/world/launcher gameplay run were not performed in this environment.
The changed offline transport fixture is type-checked with Windows tests; it is
not reported as executed on hardware here.

Changed presentation files: `horizon-client-d3d9/device.rs` owns swapchains, depth,
cached IBs and shader state; `screen.rs`/`gui_device.rs` own the projected GUI stream;
`fog.rs`/`fog_program.rs` contain the
SDK-generated fog program; `terrain.rs`, `terrain_device.rs`, `terrain_program.rs`
implement hardware-only compact vertices; `animation.rs` caches prepared pixels;
`geometry.rs` compresses CPU shadows; `texture_pages.rs` packs basic alpha ranges
and caches page order; `tiles.rs` stores
virtual atlas tiles; `atlas.rs` preserves transparent RGB. The optional development
smoke example remains in source but is no longer built/shipped by the client package. In `horizon-client-render`, only
`gfx.rs` helper visibility changes for normal builds. The legacy renderer and its
`atlas`, `compat`, `frame`, `resident`, `solid_cache`, `mesh` and its
`compact`, `convert`, `upload`, `arrival` helpers own culling, cached plans, shared mips, sky
and bounded CPU staging/compaction; `frame/gui_cache.rs` reuses prepared HUD ranges. Cargo only adds the already-used
glam dependency to the DX9 crate. CI and TESTING.txt carry reproducible checks. Small backend-selection, API-tooltip
and checked-download helpers moved to sibling modules to satisfy the new upstream
file-size guard without expanding already oversized files; their behavior is unchanged.

| Physical result | GMA3150 / N455 / Win7 x64 | HD4570 / Win7 x64 |
| --- | --- | --- |
| Actual driver caps and HAL rasterization | Owner game log: Chell 1.8b, vendor 8086/device a011, PS2, VS0, DevCaps 8ae50, 2048², 8 samplers/stages, HAL SWVP, X8R8G8B8/D24X8 | Pending user log |
| Title/menu/world/input/save, resize/Alt+Tab/fullscreen | Owner ran title/world/inventory at two chunks; remaining transitions not certified | Not tested |
| Screenshots and visual comparison | Owner supplied flat/normal world screenshots; ice issue reported | Not captured |
| Median/1% low, CPU stages, draw/bind/upload/memory comparison | Owner .hzperf/log available; new-build comparison pending | Not measured |

See `TESTING.txt` in the archive for the short game test and diagnostic variables.
The log prints adapter/vendor/device, PS/VS versions, DevCaps, texture limits, stages,
vertex mode and formats, plus periodic CPU/Present/draw/bind/upload/residency counters.
The owner has established a physical GMA game run for the preceding build. Remaining
visual/lifecycle coverage and this update still need hardware retests; neither CI,
Wine nor VM results substitute for those.
