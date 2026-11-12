# Windows 7 Direct3D9 client preview

Base audited: `fd37e40220151e9279745d8e332b5d58fbea0041` (this fork's main,
11 November 2026), rather than the specification's older `36fc4fa1`.
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
and 16 MiB–1 GiB. It no longer imposes the GMA-scale 128 MiB ceiling on an RTX.
Render distance and texture quality are never rewritten. CPU shadows, physical
arena allocations, live GPU vertices and decode/index scratch are reported separately.

Sprite pages group native resolutions, undoing only array upscaling (a 16 px sprite
in a 64 px CPU array keeps its original detail). Animated sprites have separate
pages at most 256 px, except when the sprite itself needs more space. Static pages
are at most 1024 px, subject to caps. GUI uses sparse pages at most 512 px. This
bounds the area a driver can dirty/copy for small animation/glyph updates. Per-sprite
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

## RTX3050 investigation

Owner-reported progression: about 12 FPS initially, 50 at 16 chunks after the first
fix, and about 100 at 16 in `de0ecaeb`. These are prior-build observations, not
measurements of the code in this update. The latest valid 20-second control report
contains 88.61 average FPS, frame p99 16.66 ms and maximum 19.63 ms; that capture does
not include the resize stall. The DX9 log separately records a 203.85 ms CPU peak,
up to 114,669,760 transient vertex bytes in a frame after Reset, and roughly
8-10 MiB of index uploads even when warm. The memory snapshot reports 1,585.6 MiB
resident, 539.7 MiB of requested terrain VBs, 146.9 MiB of compressed shadows and
37.0 MiB of index/decode scratch. Requested GPU storage is not measured physical VRAM.

This update removes resize-driven mesh invalidation and repeated cold streaming,
reuses the existing CPU portal/column culling, retains visibility indices, compacts
fragmented immutable VBs off-thread and improves lossless shadow compression.
Texture loading now reuses the normal mip generator, retaining solidified hidden
RGB for fast leaves. Sky and terrain fog use their distinct existing Frame inputs.
Hardware speed/RAM gains and shader/resize screenshots still require a fresh owner run.

The log/F3 report CPU timings, not GPU timestamps. `horizon-ctl.exe` provides the
existing opt-in `bench 20`, `memory` and `trace render-trace.json 10` commands; see
TESTING.txt. New log fields count successful compactions. `memory` lists retained
managed visibility IBs separately. The Win7 executable offers DX9 in the stock API
row and retains other saved API preferences; this ZIP currently contains DX9 only.
A unified executable still requires runtime dispatch and lazy modern API imports.

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

The archive contains `horizon-client.exe`, `d3d9-smoke.exe`, `horizon-ctl.exe`, `TESTING.txt` and exact
build information. Select the client exe in the existing Win7 launcher's native-client
setting. Direct launch: `horizon-client.exe --resources C:\path\to\26.3 --game-dir C:\path\to\test-game`.
Resources and saves are never distributed in this archive.

This iteration has ten CPU tests (including shader-token arithmetic and exact shadow
bits) and eleven PE policy/parser tests. The Win7 renderer and smoke are cross-checked;
the packaging workflow builds the complete client/control/smoke ZIP and audits static/delay imports,
architecture and subsystem version. Compiler warnings inherited from the current
nightly/native dependencies are separate from a successful check. Full parity harnesses
and a resource/world/launcher gameplay run were not performed in this environment.

Changed presentation files: `horizon-client-d3d9/device.rs` owns swapchains, depth,
cached IBs and PS2 state; `fog.rs` contains the embedded fog program; `geometry.rs`
compresses CPU shadows; `atlas.rs` preserves transparent RGB; `examples/smoke.rs`
exercises the new cached draw/fog/resize paths. In `horizon-client-render`, only
`gfx.rs` helper visibility changes for normal builds. The legacy renderer and its
`atlas`, `compat`, `frame`, `mesh` and `mesh/compact` modules own culling, cached
plans, shared mips, sky and bounded compaction. Cargo only adds the already-used
glam dependency to the DX9 crate. CI and TESTING.txt carry reproducible checks.

| Physical result | GMA3150 / N455 / Win7 x64 | HD4570 / Win7 x64 |
| --- | --- | --- |
| Actual driver caps and HAL rasterization | Pending user log | Pending user log |
| Title/menu/world/input/save, resize/Alt+Tab/fullscreen | Not tested | Not tested |
| Screenshots and visual comparison | Not captured | Not captured |
| Median/1% low, CPU stages, draw/bind/upload/memory comparison | Not measured | Not measured |

See `TESTING.txt` in the archive for the short device/game test and diagnostic variables.
The log prints adapter/vendor/device, PS/VS versions, DevCaps, texture limits, stages,
vertex mode and formats, plus periodic CPU/Present/draw/bind/upload/residency counters.
A physical successful game run is still required for acceptance; neither CI, Wine nor
VM results can establish GMA3150 hardware acceleration.
