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

Managed textures/VBs/IBs survive Reset. DEFAULT transient storage and bound references
are released before Reset. LOST waits for cooperative recovery; NOTRESET or resize
resets on the creating thread. Minimized windows skip drawing. Resources retain their
device/window through owned COM handles and cannot move to another thread. Reverse Z
uses a real depth surface; paired view/projection Z reflections keep the common clip
coordinates and positive fixed-function fog depth.

Each VB is bounded by 16-bit indices and caps (at most 65,532 vertices); no 256 MiB
arena pages. Changed sections upload once, then draw through frustum/Sides culling
and atlas-page batches; transparent sections draw far-to-near. Render/texture/sampler
states and unchanged matrices are cached. Texture pages contain per-sprite mip gutters;
GUI pages and standalone pictures are allocated only when needed. Panorama storage is
released on entering a world. Default memory budgets derive from the driver's advisory
available memory, with explicit environment overrides and checked allocations. An
exhausted budget produces a diagnostic, never an automatic render-distance reduction.
Managed residency numbers describe requested resources, not measured physical VRAM.

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

Evidence before physical tests: two atlas behavior tests; Win7-target checks of the
legacy client with audio, the smoke example, and the ordinary client with its default
features. The new packaging workflow builds the ZIP and audits static/delay imports,
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
