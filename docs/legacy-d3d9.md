# Windows 7 Direct3D9 client preview

Base audited: `fd37e40220151e9279745d8e332b5d58fbea0041` (this fork's main,
11 November 2026), rather than the specification's older `36fc4fa1`.
Scope: Windows 7 SP1 **x64**, Atom N455/GMA3150 first, HD4570 second. No x86 deliverable.
The ZIP's executable selects DX9 by its `legacy-d3d9` feature, without changing the
ordinary client's default renderer or any saved graphics defaults.

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
| `camera`, `light`, `fog`, `weather`, `particles`, `clouds` | Keep camera math, CPU block light, particles and weather inputs. Fixed-function stage 0 multiplies the sprite and tint/AO; stage 1 samples a 16x16 day/night lightmap. Meshes do not upload when the sun moves. Basic vertex fog replaces advanced fog. |
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
There are **no shaders**, shader bytecode, D3DCompiler or SDK runtime requirement.
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

Managed textures/IBs survive Reset. DEFAULT terrain and transient buffers and bound
references are released before Reset. Terrain handles are invalidated through weak
registrations and lazily reconstructed from the section CPU cache. LOST waits for cooperative recovery; NOTRESET or resize
resets on the creating thread. Minimized windows skip drawing. Resources retain their
device/window through owned COM handles and cannot move to another thread. Reverse Z
uses a real depth surface; paired view/projection Z reflections keep the common clip
coordinates and positive fixed-function fog depth.

Each VB is bounded by 16-bit indices and caps (at most 65,532 vertices); no 256 MiB
arena pages. Section meshes convert once into losslessly compressed CPU shadows.
The cache decodes only to load missing GPU data or draw uncached geometry. Nearby
sections share an immutable DEFAULT VB with positions local to a 128-block region;
negative coordinates use Euclidean division and the dimension's minimum Y is kept.
New pages upload once and are never modified while draws can reference them. This
also avoids placing permanent terrain in dynamic/AGP storage on discrete GPUs.

On hardware vertex processing, visible solid quad ranges from multiple sections
become one indexed batch per arena/page/alpha mode. CPU frustum and Sides culling
still select the same geometry. On software vertex processing, narrow direct draws
avoid transforming holes between sparse index ranges. Transparent sections/quads
keep their original far-to-near order. Dynamic index and transient vertex rings use
NOOVERWRITE on append and DISCARD only on wrap, rather than renaming a full buffer
for every small draw. Terrain GPU residency is bounded, with offscreen pages evicted
first; an allocation failure still permits transient draws. The automatic target is
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
| Nether/End, panorama, sun/moon | Basic sky/fog; simplified textured End sky and portal |
| Animated textures/filtering | Frame changes, nearest/linear and mip on/off; exact mip-count clamp, anisotropy, interpolation/Polytone expression modes are not implemented |
| Fancy clouds/star field, foil/glint, glowing outlines, pack core shaders | Omitted in this preview; base objects/items remain |
| LOD, GPU occlusion/culling/skinning, shader packs, PBR, HDR, post chains, TAA/FXAA, frame generation | Unavailable; no corresponding GPU pipelines start; settings remain stored |
| Split-screen compositor / GPU render capture | Explicitly unavailable; screenshots and DX9 log counters remain |

Static block-entity snapshots currently use the dynamic entity draw path. GPU caching
of those optional snapshots, finer transparent-quad sorting and CPU-transformed vertex
A/B testing remain performance follow-ups requiring hardware evidence.

## Performance investigation after the first player test

The owner reports title/world/HUD rendering on RTX3050/i5-10400, but a nearly
constant 12 FPS. The attached log records 70.8-78.0 ms renderer CPU frames while
terrain draws rise from 1,141 to 8,493. Its 1,301 uniformly upscaled 64 px layers
became one 8192-square, 357,892,096-byte managed atlas; total requested texture
storage reached 525,666,308 bytes. Animation dirtied that static atlas. These are
observations from the old build, not post-fix timings or measured physical VRAM.
The owner also reports the standalone smoke test works on GMA3150; gameplay there
and on HD4570 remains unverified.

The atlas/mesh architecture changes above address confirmed allocation amplification
and a likely driver-transfer bottleneck. The exact cause of the 75 ms wait and the
resulting FPS must be verified by the next RTX3050 run. F3 and the five-second log
now separate texture/lightmap/GUI updates, visibility, cache population, terrain,
entities, effects/hand and GUI drawing, plus section conversion outside render.
They report CPU elapsed time including driver waits, not GPU query times.
`horizon-ctl.exe` is packaged for the existing `bench 20`, `memory` and
`trace render-trace.json 10` diagnostics; TESTING.txt has the short procedure.
Native-size atlas/isolation tests are synthetic CPU tests, not GPU benchmarks.
The Win7 executable explicitly offers Direct3D9 in both API settings rows. Existing
saved API preferences remain stored; modern APIs require a different build and are
not misleadingly offered by this executable. A unified Win7-safe executable would
also need runtime backend dispatch and lazy modern API imports.

## Second RTX3050 report and regional batching

The owner reports 200+ FPS at 8 chunks and about 50 FPS at 16 after the first fix.
The supplied log confirms the sprite atlas fell to 13,357,696 bytes, but CPU mesh
shadows reached 528,287,232 bytes. A 128 MiB GPU cache stayed full while 24–100 MiB
were uploaded per frame. After resize the log captured a 2,091.7 ms solid pass and
231.6 ms translucent pass. These are old-build owner observations, not new timings.

The regional arenas, compressed shadows, adaptive cache and stream rings above
address those paths. GUI alpha batching preserves painter order at overlapping
bounds, grouping only independent glyphs/polygons; a CPU compositing test compares
its reordered output against the original image. The two independent fog intervals
are no longer mixed into a start/end pair that fills the foreground with haze.
The fixed-function approximation uses the stricter complete interval (it still does
not exactly reproduce both modern spherical/cylindrical fog curves at once).

The old F3 GPU heading double-counted the CPU frame and its own child stages. DX9
now reports only CPU leaf times under a CPU heading; real GPU query data stays absent.
The attached memory.json and frame-stats.json contain connection errors because the
previous instructions omitted --control. TESTING.txt now gives the correct launch
command. The game does not silently enable a control endpoint. Automatic log windows
include CPU average/peak and the worst frame's stages, so stalls remain diagnosable
without a control connection. Updated FPS, RAM and fog screenshots await owner testing.

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

The archive contains `horizon-client.exe`, `d3d9-smoke.exe`, `TESTING.txt` and exact
build information. Select the client exe in the existing Win7 launcher's native-client
setting. Direct launch: `horizon-client.exe --resources C:\path\to\26.3 --game-dir C:\path\to\test-game`.
Resources and saves are never distributed in this archive.

Evidence before physical tests: two atlas behavior tests and eleven PE policy/parser tests; Win7-target checks of the
legacy client with audio, the smoke example, and the ordinary client with its default
features (also checked on the standard `x86_64-pc-windows-gnu` ABI after the Win7
dependency separation). The new packaging workflow builds the ZIP and audits static/delay imports,
architecture and subsystem version. Compiler warnings inherited from the current
nightly/native dependencies are separate from a successful check. Full parity harnesses
and a resource/world/launcher gameplay run were not performed in this environment.

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
