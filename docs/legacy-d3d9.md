# Windows 7 Direct3D9 client preview

Current upstream integration: `298c01b0206d72294b25753c9f8d0a77b742b3bb` (this fork's main,
13 November 2026). Merged into the existing development branch without rewriting its history.
Scope: Windows 7 SP1 **x64**, Atom N455/GMA3150 first, HD4570 second. No x86 deliverable.
The ZIP's executable selects DX9 by its `legacy-d3d9` feature, without changing other backend implementations or any saved graphics defaults.

This is a hardware-test preview. Successful compilation and import inspection do
not establish that the title screen or a world works on either physical GPU.

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
Vertices always use the fixed-function pipeline. An embedded `ps_2_0` program combines the two fog curves when PS2 and texture-coordinate generation are available; otherwise a fixed-function approximation is logged. No VS program, D3DCompiler or SDK runtime is required.
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
sections share an immutable DEFAULT VB with positions local to a 128-block region;
negative coordinates use Euclidean division and the dimension's minimum Y is kept.
New pages upload once and are never modified while draws can reference them. This
also avoids placing permanent terrain in dynamic/AGP storage on discrete GPUs.

On hardware vertex processing, visible solid quad ranges from multiple sections
become one indexed batch per arena/page/alpha mode. The existing CPU column/frustum walk, vanilla portal visibility graph and Sides culling select geometry; spectator bypass is preserved. Visibility index buffers are retained until their canonical quad ranges change, with one bounded index scratch instead of permanent per-batch CPU index arrays. On software vertex processing, narrow direct draws
avoid transforming holes between sparse index ranges. Transparent sections/quads
keep their original far-to-near order. Dynamic index and transient vertex rings use
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
of those optional snapshots, finer transparent-quad sorting and CPU-transformed vertex
A/B testing remain performance follow-ups requiring hardware evidence.

## Current optimization and profiling pass

The owner reports 580+ FPS at 16 chunks. Their attached 20-second capture contains
507.09 average FPS, frame p50/p95/p99 1.833/2.633/4.262 ms, and maximum 71.243 ms.
Average DX9 CPU frame time is 1.746 ms. A separate stage snapshot shows GUI 0.345 ms
(one draw, one texture bind, one vertex-buffer lock), terrain plan preparation
0.326 ms, solid draws 0.254 ms, translucent draws 0.131 ms and visibility 0.102 ms.
Memory capture: 1116.4 MiB process resident; 261.6 MiB requested terrain GPU storage,
238.5 MiB live vertices; 68.2 MiB compressed CPU shadows. Requested GPU storage is
not a measurement of physical VRAM, and allocator commit is not process resident.
DX9's elapsed CPU stages must not be compared directly to the other backends' GPU
timestamp labels.

The owner's separate 48-chunk comparison is DX9 80 FPS/3240 MB, DX12 244/1570,
Vulkan 415/1350, OpenGL 201/1480. The attached capture is at 16 chunks; it does not
establish the cause of the 48-chunk gap. None of these numbers measures this update.

The shared wgpu renderer backs Vulkan, DX12 and OpenGL. Reused mechanisms are its
CPU column/frustum/portal culling, dense Resident storage, model data and mip generator.
Its compute/indirect draw and shader-expanded packed quads cannot run on baseline
D3D9/SM2. This update therefore adds compatible storage and submission paths:

- One dense Resident<Section> replaces a second section hash store. Empty sections
  retain only a presence key; block-entity-only sections retain their CPU data.
  Visible slots access the draw data directly.
- An exact solid-plan cache compares section identities, revisions and camera-facing
  ranges. Unchanged plans reuse their cached indices; resource/page/reset changes
  invalidate them. There is no lossy fingerprint or stale-frustum shortcut.
- Hardware VS2 devices can use exact 20-byte terrain vertices or 28-byte vertices
  for models with arbitrary floating-point position/UV. Unrepresentable values
  retain the 32-byte format. CPU shadows encode the same smaller layout and expand
  losslessly for fallback/recovery. The SDK-generated vs_2_0 program uses 12
  arithmetic slots. Shader/declaration creation must succeed, and PS2 fog must be
  available. GMA/software vertex processing keeps the original FFP format, with
  no software vertex-shader requirement.
- Initial section conversion and shadow/vertex encoding share the existing single
  compaction worker. A conservative 8 MiB charge bounds pending source/results;
  overflow and replacements/edits use the synchronous path. COM uploads stay on
  the render thread. Tickets and a world epoch reject obsolete results. Reusable
  scratch is separate from that charge; one recycled vertex vector is capped at
  2 MiB. This does not change the shared streaming worker policy.
- Animation mip/extrusion pixels have a 4 MiB payload LRU cache. Misses use the
  existing mip generator; hits preserve its bytes and avoid regenerating them.
  Atlas placement is resolved once per terrain quad instead of once per vertex.
- The F3 axis crosshair consumes the existing debug_crosshair frame data after
  screen overlays and before the HUD, using the hand projection without bob/fog.

Logs distinguish compact20/wide28/ffp32 bytes, uncached_visible_bytes,
solid_plan_reused, staging queue/conversion wall time and animation-cache hits.
These counters permit diagnosis of budget fallback, upload pressure and cold work
without attributing all delay to drawing. Cache targets are advisory; allocation
failures keep the existing drawable fallback. Settings files are not rewritten.

The upstream .hzperf profiler is useful here: DX9 phases and section-worker spans
now feed its timeline. Windows supports frame/timeline/memory capture; the CPU
stack/heap sampler currently exists only on Linux. DX9 GPU timestamps remain
explicitly unavailable, rather than being populated with CPU milliseconds. Use
`horizon-ctl profile client start --seconds 20 --timeline --no-samples`, and use the capture path from the start reply/log. `status` describes an active
capture; `stop` can end it before the automatic 20-second stop. Capture
is opt-in and nothing is shared automatically. The old trace command now also
writes .hzperf, not Chrome JSON. See TESTING.txt for complete launch commands.

The supplied RTX log confirms acceptance of the SDK-compiled ps_2_0 fog program.
F5 skin depth and fast-leaf colour fixes are in the preceding iteration. Common
resource preparation before device creation remains outside this work. Remaining
work includes real 48-chunk measurements, transparency draw overhead and further
GUI CPU preparation reductions; parity with OpenGL has not been established.

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

This iteration has fifteen CPU tests (including tile seams/placement, executable SM2 arithmetic,
all vertex layouts/partial codec blocks and animation-cache eviction) and eleven PE policy/parser tests. The packaging workflow builds
the client/control ZIP and audits static/delay imports,
architecture and subsystem version. Compiler warnings inherited from the current
nightly/native dependencies are separate from a successful check. Full parity harnesses
and a resource/world/launcher gameplay run were not performed in this environment.

Changed presentation files: `horizon-client-d3d9/device.rs` owns swapchains, depth,
cached IBs, staged GUI buffers and shader state; `fog.rs`/`fog_program.rs` contain the
SDK-generated fog program; `terrain.rs`, `terrain_device.rs`, `terrain_program.rs`
implement hardware-only compact vertices; `animation.rs` caches prepared pixels;
`geometry.rs` compresses CPU shadows; `tiles.rs` stores
virtual atlas tiles; `atlas.rs` preserves transparent RGB. The optional development
smoke example remains in source but is no longer built/shipped by the client package. In `horizon-client-render`, only
`gfx.rs` helper visibility changes for normal builds. The legacy renderer and its
`atlas`, `compat`, `frame`, `resident`, `solid_cache`, `mesh` and its
`compact`, `convert`, `upload` helpers own culling, cached plans, shared mips, sky
and bounded CPU staging/compaction. Cargo only adds the already-used
glam dependency to the DX9 crate. CI and TESTING.txt carry reproducible checks. Small backend-selection, API-tooltip
and checked-download helpers moved to sibling modules to satisfy the new upstream
file-size guard without expanding already oversized files; their behavior is unchanged.

| Physical result | GMA3150 / N455 / Win7 x64 | HD4570 / Win7 x64 |
| --- | --- | --- |
| Actual driver caps and HAL rasterization | Owner reports smoke passed; caps log pending | Pending user log |
| Title/menu/world/input/save, resize/Alt+Tab/fullscreen | Not tested | Not tested |
| Screenshots and visual comparison | Not captured | Not captured |
| Median/1% low, CPU stages, draw/bind/upload/memory comparison | Not measured | Not measured |

See `TESTING.txt` in the archive for the short game test and diagnostic variables.
The log prints adapter/vendor/device, PS/VS versions, DevCaps, texture limits, stages,
vertex mode and formats, plus periodic CPU/Present/draw/bind/upload/residency counters.
A physical successful game run is still required for acceptance; neither CI, Wine nor
VM results can establish GMA3150 hardware acceleration.
