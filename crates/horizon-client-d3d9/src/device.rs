//! All COM and locked-memory operations stay here. Resources keep their device and
//! window alive and are deliberately !Send/!Sync: D3D9 Reset belongs to its creator.
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
    rc::{Rc, Weak},
    sync::Arc,
};
use windows::{
    Foundation::Numerics::Matrix4x4,
    Win32::{
        Foundation::{BOOL, HWND, RECT},
        Graphics::Direct3D9::*,
    },
    core::{HRESULT, Interface},
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const D3DERR_DEVICELOST: HRESULT = HRESULT(0x88760868u32 as i32);
const D3DERR_DEVICENOTRESET: HRESULT = HRESULT(0x88760869u32 as i32);
const D3DTS_WORLD: D3DTRANSFORMSTATETYPE = D3DTRANSFORMSTATETYPE(256);
const FVF: u32 = D3DFVF_XYZ | D3DFVF_DIFFUSE | D3DFVF_TEX2;
const MAX_QUADS: u32 = 16_383;
use crate::{EncodedVertices, Fog, IndexBatch, Ring, Vertex, VertexEncoding};
// Evaluate the original COM expression exactly once, with unchanged error flow.
// The normal build expands to just that expression, without a counter branch.
macro_rules! profile_call {
    ($inner:expr, $api:ident, $call:expr) => {{
        #[cfg(feature = "submit-profile")]
        $inner
            .profile
            .count(|c| c.calls[crate::submit_profile::Api::$api as usize] += 1);
        let result = $call;
        #[cfg(feature = "submit-profile")]
        if result.is_err() {
            $inner
                .profile
                .count(|c| c.failures[crate::submit_profile::Api::$api as usize] += 1);
        }
        result
    }};
}
macro_rules! profile_count {
    ($inner:expr, $update:expr) => {
        #[cfg(feature = "submit-profile")]
        $inner.profile.count($update);
    };
}
#[path = "terrain_device.rs"]
mod terrain_device;
use terrain_device::TerrainPipeline;
#[path = "gui_device.rs"]
mod gui_device;
#[path = "presentation_formats.rs"]
mod presentation_formats;
#[path = "texture_levels.rs"]
mod texture_levels;
#[path = "world_target.rs"]
mod world_target;
pub use gui_device::ScreenTriangles;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    Alpha,
    Additive,
    Invert,
    Multiply,
    Cracks,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawState {
    pub blend: Blend,
    pub depth_test: bool,
    pub depth_write: bool,
    /// World uses reverse Z; GUI model depth uses the normal 0-near/1-far convention.
    pub reverse_z: bool,
    pub alpha_test: bool,
    pub alpha_ref: u8,
    pub cull: bool,
    pub lightmap: bool,
    /// 0 ordinary, 1 item foil, 2 armour foil; slot one is the repeating glint.
    pub glint: u8,
    /// Entity overlay texel in RGBA byte order. Alpha is the retained base colour.
    pub hurt_overlay: Option<u32>,
    pub equal_depth: bool,
    pub fog: Option<Fog>,
    pub fog_program: crate::FogProgram,
    pub scissor: Option<[u32; 4]>,
    pub linear: bool,
    pub mipmaps: bool,
    pub wrap: bool,
}
impl Default for DrawState {
    fn default() -> Self {
        Self {
            blend: Blend::Opaque,
            depth_test: true,
            depth_write: true,
            reverse_z: true,
            alpha_test: true,
            alpha_ref: 127,
            cull: true,
            lightmap: true,
            glint: 0,
            hurt_overlay: None,
            equal_depth: false,
            fog: None,
            fog_program: crate::FogProgram::Dual,
            scissor: None,
            linear: false,
            mipmaps: true,
            wrap: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Caps {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub max_texture: u32,
    pub max_primitive_count: u32,
    pub available_texture_bytes: u32,
    pub software_vertices: bool,
    pub color_bits: u8,
    pub raw: D3DCAPS9,
}
type MeshHandle = RefCell<Option<IDirect3DVertexBuffer9>>;
struct Inner {
    #[cfg(feature = "submit-profile")]
    profile: crate::submit_profile::Profile,
    stream_sequence: Cell<u64>,
    default_meshes: RefCell<Vec<Weak<MeshHandle>>>,
    presentation: RefCell<Option<Presentation>>,
    world: RefCell<world_target::State>,
    device: IDirect3DDevice9,
    _d3d: IDirect3D9,
    _window: Arc<Window>,
}
struct Presentation {
    chain: IDirect3DSwapChain9,
    _depth: IDirect3DSurface9,
    width: u32,
    height: u32,
    interval: u32,
}
pub struct Texture {
    raw: IDirect3DTexture9,
    base: IDirect3DBaseTexture9,
    owner: Rc<Inner>,
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub bytes: u64,
    mip_backup: Option<texture_levels::Backup>,
}
pub struct Mesh {
    raw: Rc<MeshHandle>,
    owner: Rc<Inner>,
    pub vertices: u32,
    filled: u32,
    encoding: VertexEncoding,
}
/// Immutable visibility indices. CPU/managed storage survives a real device Reset;
/// the runtime owns the small CPU copy instead of a second application Vec.
pub struct Indices {
    raw: IDirect3DIndexBuffer9,
    owner: Rc<Inner>,
    windows: Vec<crate::geometry::IndexWindow>,
    count: u32,
}
impl Indices {
    pub fn bytes(&self) -> u64 {
        u64::from(self.count) * 2
    }
    pub fn cpu_bytes(&self) -> usize {
        self.windows.capacity() * size_of::<crate::geometry::IndexWindow>()
    }
}
impl Mesh {
    /// Reset invalidates DEFAULT buffers; their owner can lazily upload its CPU data again.
    pub fn resident(&self) -> bool {
        self.raw.borrow().is_some()
    }
    pub fn bytes(&self) -> u64 {
        u64::from(self.vertices) * self.stride() as u64
    }
    pub fn stride(&self) -> usize {
        self.encoding.stride()
    }
    pub fn encoding(&self) -> VertexEncoding {
        self.encoding
    }
}
struct StateCache {
    render: [Option<u32>; 256],
    stage: [[Option<u32>; 33]; 3],
    sampler: [[Option<u32>; 14]; 3],
    textures: [usize; 3],
    stream: crate::stream_cache::StreamCache,
}
impl Default for StateCache {
    fn default() -> Self {
        Self {
            render: [None; 256],
            stage: [[None; 33]; 3],
            sampler: [[None; 14]; 3],
            textures: [usize::MAX; 3],
            stream: crate::stream_cache::StreamCache::default(),
        }
    }
}
impl StateCache {
    #[inline(always)]
    fn bind_stream(
        &mut self,
        inner: &Inner,
        buffer: &IDirect3DVertexBuffer9,
        stride: u32,
        enabled: bool,
    ) -> windows::core::Result<()> {
        profile_count!(inner, |c| c.stream_requests += 1);
        let binding = crate::stream_cache::Binding {
            buffer: buffer.as_raw() as usize,
            offset: 0,
            stride,
        };
        // SetStreamSource retains the bound VB in D3D9. Its non-owning identity
        // cannot be recycled while still bound. All stream-0 setters use this
        // path except Reset's null bind, which explicitly invalidates it.
        self.stream.bind(binding, enabled, || {
            // SAFETY: the caller validated the owned live VB and its layout;
            // this method only omits a setter for an identical native binding.
            unsafe {
                profile_call!(
                    inner,
                    SetStreamSource,
                    inner.device.SetStreamSource(0, buffer, 0, stride)
                )
            }
        })
    }
    fn render(
        &mut self,
        inner: &Inner,
        state: D3DRENDERSTATETYPE,
        value: u32,
    ) -> windows::core::Result<()> {
        let device = &inner.device;
        profile_count!(inner, |c| c.cache_requests[0] += 1);
        let slot = &mut self.render[state.0 as usize];
        if *slot != Some(value) {
            // SAFETY: a live owned device, with a documented render-state value.
            unsafe {
                profile_call!(inner, SetRenderState, device.SetRenderState(state, value))?;
            }
            *slot = Some(value);
        }
        Ok(())
    }
    fn stage(
        &mut self,
        inner: &Inner,
        index: u32,
        state: D3DTEXTURESTAGESTATETYPE,
        value: u32,
    ) -> windows::core::Result<()> {
        let device = &inner.device;
        profile_count!(inner, |c| c.cache_requests[2] += 1);
        let slot = &mut self.stage[index as usize][state.0 as usize];
        if *slot != Some(value) {
            // SAFETY: stages 0/1 were checked in caps; device and constants are live.
            unsafe {
                profile_call!(
                    inner,
                    SetTextureStageState,
                    device.SetTextureStageState(index, state, value)
                )?;
            }
            *slot = Some(value);
        }
        Ok(())
    }
    fn sampler(
        &mut self,
        inner: &Inner,
        index: u32,
        state: D3DSAMPLERSTATETYPE,
        value: u32,
    ) -> windows::core::Result<()> {
        let device = &inner.device;
        profile_count!(inner, |c| c.cache_requests[1] += 1);
        let slot = &mut self.sampler[index as usize][state.0 as usize];
        if *slot != Some(value) {
            // SAFETY: samplers 0/1 and their D3D9 values were checked in caps.
            unsafe {
                profile_call!(
                    inner,
                    SetSamplerState,
                    device.SetSamplerState(index, state, value)
                )?;
            }
            *slot = Some(value);
        }
        Ok(())
    }
}
/// One frame's transient vertices. It owns no DEFAULT resource, so retaining this
/// token cannot obstruct Reset. A later upload/begin invalidates it explicitly.
pub struct StagedTriangles {
    owner: Rc<Inner>,
    serial: u64,
    first: u32,
    count: u32,
}
/// A token only: it retains no DEFAULT buffer and cannot obstruct Reset.
pub struct StagedQuads {
    owner: Rc<Inner>,
    serial: u64,
    first: u32,
    count: u32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PixelProgram {
    Fog(crate::FogProgram),
    Hurt { fog: bool },
}

impl PixelProgram {
    fn fog(self) -> Option<crate::FogProgram> {
        match self {
            Self::Fog(mode) => Some(mode),
            Self::Hurt { fog: true } => Some(crate::FogProgram::Dual),
            Self::Hurt { fog: false } => None,
        }
    }
}

pub struct Device {
    inner: Rc<Inner>,
    pub caps: Caps,
    present: D3DPRESENT_PARAMETERS,
    indices: Option<IDirect3DIndexBuffer9>,
    transient: Option<IDirect3DVertexBuffer9>,
    transient_capacity: usize,
    transient_ring: Ring,
    transient_serial: u64,
    screen: gui_device::Stream,
    screen_active: bool,
    dynamic_indices: Option<IDirect3DIndexBuffer9>,
    index_ring: Ring,
    using_dynamic_indices: bool,
    reset_pending: bool,
    surface_pending: bool,
    lost: bool,
    scene: bool,
    anisotropy: u32,
    #[cfg(feature = "dev-diagnostics")]
    benchmark_slot: Option<crate::benchmark::Slot>,
    #[cfg(feature = "dev-diagnostics")]
    sampler_history_valid: bool,
    cached_state: Option<(DrawState, usize, usize)>,
    state_cache: StateCache,
    cached_transform: Option<[[f32; 16]; 3]>,
    fog_shader: Option<IDirect3DPixelShader9>,
    fog_variants: [Option<IDirect3DPixelShader9>; 4],
    hurt_shaders: [Option<IDirect3DPixelShader9>; 2],
    shader_active: Option<PixelProgram>,
    fog_constants: Option<Fog>,
    terrain: Option<TerrainPipeline>,
    terrain_encoding: Option<VertexEncoding>,
    vertex_constants: [[f32; 4]; 8],
    vertex_constants_dirty: bool,
    pub draws: u64,
    pub fog_draws: [u64; 5],
    pub binds: u64,
    pub uploads: u64,
    pub resets: u64,
    pub mesh_uploads: u64,
    pub stream_uploads: u64,
    pub stream_locks: u64,
    pub index_uploads: u64,
}
impl Device {
    #[inline(always)]
    pub fn profile_begin_frame(&self) {
        #[cfg(feature = "submit-profile")]
        self.inner.profile.begin();
    }
    #[inline(always)]
    pub fn profile_pass(&self, _pass: crate::SubmitPass) {
        #[cfg(feature = "submit-profile")]
        self.inner.profile.pass(_pass);
    }
    #[inline(always)]
    pub fn profile_end_frame(&self, _presented: bool) {
        #[cfg(feature = "submit-profile")]
        self.inner.profile.end(_presented);
    }
    #[inline(always)]
    pub fn log_submit_profile(&self) {
        #[cfg(feature = "dev-diagnostics")]
        if let Some(slot) = self.benchmark_slot {
            log::info!("{}", slot.marker());
        }
        #[cfg(feature = "submit-profile")]
        self.inner.profile.log();
    }
    #[cfg(feature = "dev-diagnostics")]
    pub fn benchmark_active(&self, trial: crate::benchmark::Trial) -> bool {
        self.benchmark_slot
            .is_some_and(|slot| slot.candidate && slot.trial == trial)
    }
    #[cfg(feature = "dev-diagnostics")]
    pub fn set_benchmark_slot(&mut self, slot: Option<crate::benchmark::Slot>) -> bool {
        if self.benchmark_slot == slot {
            return false;
        }
        // Close the preceding counter window before changing its label.
        self.log_submit_profile();
        self.benchmark_slot = slot;
        if !self.benchmark_active(crate::benchmark::Trial::SamplerReuse) {
            self.sampler_history_valid = false;
        }
        true
    }
    #[cfg(feature = "dev-diagnostics")]
    pub fn profile_solid_face_groups(&self, count: u32) {
        profile_count!(self.inner, |c| c.solid_face_groups_culled +=
            u64::from(count));
    }
    #[cfg(feature = "dev-diagnostics")]
    pub fn profile_glint_locks_saved(&self, count: usize) {
        profile_count!(self.inner, |c| c.glint_locks_saved += count as u64);
    }
    pub fn fog_parameters(&self) -> Option<Fog> {
        self.fog_constants
    }
    fn record_draw(&mut self) {
        self.draws += 1;
        if let Some(mode) = self.shader_active.and_then(PixelProgram::fog) {
            self.fog_draws[mode as usize] += 1;
        }
    }
    pub fn new(window: Arc<Window>, vsync: bool) -> Result<Self> {
        Self::new_with_options(window, vsync, crate::Preferences::default())
    }
    pub fn new_with_options(
        window: Arc<Window>,
        vsync: bool,
        options: crate::Preferences,
    ) -> Result<Self> {
        match Self::create(window.clone(), vsync, options) {
            Err(error)
                if options.color_depth == crate::ColorDepth::Bits16
                    && error.downcast_ref::<windows::core::Error>().is_some() =>
            {
                // Some legacy drivers advertise a format pair that allocation
                // then rejects. Retry a native 32-bit surface, never an extra
                // conversion/render pass or a CPU rasterizer.
                log::warn!(
                    "D3D9 16-bit device/surface creation failed: {error}; retrying 32-bit color"
                );
                Self::create(
                    window,
                    vsync,
                    crate::Preferences {
                        color_depth: crate::ColorDepth::Bits32,
                        ..options
                    },
                )
            }
            result => result,
        }
    }
    fn create(window: Arc<Window>, vsync: bool, options: crate::Preferences) -> Result<Self> {
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            return Err("Direct3D9 requires a Win32 HWND".into());
        };
        let hwnd = HWND(handle.hwnd.get() as *mut c_void);
        // SAFETY: D3D_SDK_VERSION is the runtime ABI; no user pointer is passed.
        let d3d = unsafe { Direct3DCreate9(D3D_SDK_VERSION) }.ok_or(
            "Direct3DCreate9 failed (install the display manufacturer's Windows 7 driver)",
        )?;
        let mut raw = D3DCAPS9::default();
        let mut id = D3DADAPTER_IDENTIFIER9::default();
        let mut mode = D3DDISPLAYMODE::default();
        // SAFETY: all output structures are live, writable and correctly sized.
        unsafe {
            d3d.GetDeviceCaps(0, D3DDEVTYPE_HAL, &mut raw)?;
            d3d.GetAdapterIdentifier(0, 0, &mut id)?;
            d3d.GetAdapterDisplayMode(0, &mut mode)?;
        }
        let name = String::from_utf8_lossy(
            &id.Description
                .iter()
                .map(|&b| b as u8)
                .take_while(|&b| b != 0)
                .collect::<Vec<_>>(),
        )
        .into_owned();
        // No vendor allowlist. A nonaccelerated desktop driver does not have HWRASTERIZATION.
        if raw.DevCaps & D3DDEVCAPS_HWRASTERIZATION as u32 == 0 {
            return Err(format!("D3D9 HAL {name}: no hardware rasterizer (DevCaps={:#x}); REF/software GPU fallback is disabled", raw.DevCaps).into());
        }
        if raw.MaxSimultaneousTextures < 2
            || raw.MaxTextureBlendStages < 2
            || raw.TextureOpCaps & D3DTEXOPCAPS_MODULATE as u32 == 0
            || raw.AlphaCmpCaps & D3DPCMPCAPS_GREATER as u32 == 0
            || raw.SrcBlendCaps & D3DPBLENDCAPS_SRCALPHA as u32 == 0
            || raw.DestBlendCaps & D3DPBLENDCAPS_INVSRCALPHA as u32 == 0
        {
            return Err(format!("D3D9 HAL {name}: missing two texture stages, MODULATE, alpha test or alpha blend; caps={raw:?}").into());
        }
        let (backbuffer, depth) =
            presentation_formats::choose(&d3d, mode.Format, options.color_depth)?;
        // SAFETY: read-only capability probes on the adapter's desktop format.
        unsafe {
            d3d.CheckDeviceFormat(
                0,
                D3DDEVTYPE_HAL,
                mode.Format,
                0,
                D3DRTYPE_TEXTURE,
                D3DFMT_A8R8G8B8,
            )?;
        }
        let requested = std::env::var("HORIZON_D3D9_VERTEX_PROCESSING").unwrap_or_default();
        let vp = if requested.is_empty() {
            options.vertex_processing
        } else {
            crate::VertexProcessing::parse(&requested)
                .ok_or("HORIZON_D3D9_VERTEX_PROCESSING must be auto, hardware or software")?
        };
        let software_vertices =
            vp.software(raw.DevCaps & D3DDEVCAPS_HWTRANSFORMANDLIGHT as u32 != 0)?;
        let color_bits = if backbuffer == D3DFMT_R5G6B5 { 16 } else { 32 };
        if options.color_depth == crate::ColorDepth::Bits16 && color_bits != 16 {
            log::warn!(
                "D3D9: RGB565 has no native windowed conversion with the preferred depth format; using 32-bit color"
            );
        }
        let size = window.inner_size();
        let present = D3DPRESENT_PARAMETERS {
            BackBufferWidth: size.width.max(1),
            BackBufferHeight: size.height.max(1),
            BackBufferFormat: backbuffer,
            BackBufferCount: 1,
            MultiSampleType: D3DMULTISAMPLE_NONE,
            SwapEffect: D3DSWAPEFFECT_DISCARD,
            hDeviceWindow: hwnd,
            Windowed: BOOL(1),
            EnableAutoDepthStencil: BOOL(0),
            AutoDepthStencilFormat: depth,
            PresentationInterval: if vsync {
                D3DPRESENT_INTERVAL_ONE as u32
            } else {
                D3DPRESENT_INTERVAL_IMMEDIATE as u32
            },
            ..Default::default()
        };
        // The implicit swapchain is only a reset anchor. Window resize replaces
        // an additional swapchain and depth surface, preserving all terrain VBs.
        let mut primary = present;
        primary.BackBufferWidth = 1;
        primary.BackBufferHeight = 1;
        primary.PresentationInterval = D3DPRESENT_INTERVAL_IMMEDIATE as u32;
        let vp = if software_vertices {
            D3DCREATE_SOFTWARE_VERTEXPROCESSING
        } else {
            D3DCREATE_HARDWARE_VERTEXPROCESSING
        };
        let mut device = None;
        // SAFETY: the Arc keeps the HWND alive; this thread owns the device and its Reset calls.
        unsafe {
            d3d.CreateDevice(
                0,
                D3DDEVTYPE_HAL,
                hwnd,
                vp as u32 | D3DCREATE_FPU_PRESERVE as u32,
                &mut primary,
                &mut device,
            )?;
        }
        let device = device.ok_or("D3D9 CreateDevice returned no device")?;
        // SAFETY: advisory query on the live device, never treated as an allocation guarantee.
        let available_texture_bytes = unsafe { device.GetAvailableTextureMem() };
        let caps = Caps {
            name,
            vendor: id.VendorId,
            device: id.DeviceId,
            max_texture: raw.MaxTextureWidth.min(raw.MaxTextureHeight),
            max_primitive_count: raw.MaxPrimitiveCount,
            available_texture_bytes,
            software_vertices,
            color_bits,
            raw,
        };
        log::info!(
            "D3D9 HAL adapter={} vendor={:#06x} device={:#06x} PS={:#x} VS={:#x} DevCaps={:#x} texture={}x{} samplers={} stages={} VP={} backbuffer={:?} depth={:?} available_bytes={}",
            caps.name,
            caps.vendor,
            caps.device,
            raw.PixelShaderVersion,
            raw.VertexShaderVersion,
            raw.DevCaps,
            raw.MaxTextureWidth,
            raw.MaxTextureHeight,
            raw.MaxSimultaneousTextures,
            raw.MaxTextureBlendStages,
            if software_vertices {
                "software (HAL rasterization)"
            } else {
                "hardware"
            },
            backbuffer,
            depth,
            available_texture_bytes
        );
        let mut result = Self {
            inner: Rc::new(Inner {
                #[cfg(feature = "submit-profile")]
                profile: crate::submit_profile::Profile::default(),
                stream_sequence: Cell::new(0),
                default_meshes: RefCell::new(Vec::new()),
                presentation: RefCell::new(None),
                world: RefCell::new(world_target::State::default()),
                device,
                _d3d: d3d,
                _window: window,
            }),
            caps,
            present,
            indices: None,
            transient: None,
            transient_capacity: 0,
            transient_ring: Ring::default(),
            transient_serial: 0,
            screen: gui_device::Stream::default(),
            screen_active: false,
            dynamic_indices: None,
            index_ring: Ring::default(),
            using_dynamic_indices: false,
            reset_pending: false,
            surface_pending: true,
            lost: false,
            scene: false,
            anisotropy: 1,
            #[cfg(feature = "dev-diagnostics")]
            benchmark_slot: None,
            #[cfg(feature = "dev-diagnostics")]
            sampler_history_valid: false,
            cached_state: None,
            state_cache: StateCache::default(),
            cached_transform: None,
            fog_shader: None,
            fog_variants: [None, None, None, None],
            hurt_shaders: [None, None],
            shader_active: None,
            fog_constants: None,
            terrain: None,
            terrain_encoding: None,
            vertex_constants: [[0.0; 4]; 8],
            vertex_constants_dirty: true,
            draws: 0,
            fog_draws: [0; 5],
            binds: 0,
            uploads: 0,
            resets: 0,
            mesh_uploads: 0,
            stream_uploads: 0,
            stream_locks: 0,
            index_uploads: 0,
        };
        if crate::development_build() {
            log::info!(
                "D3D9 stream0_cache enabled={} lifetime=frame invalidate_on_discard=true",
                software_vertices
            );
        }
        result.create_indices()?;
        if raw.PixelShaderVersion >= 0xffff_0200
            && raw.MaxTextureBlendStages >= 3
            && (software_vertices || raw.VertexProcessingCaps & D3DVTXPCAPS_TEXGEN as u32 != 0)
        {
            // SAFETY: the embedded program is terminated ps_2_0 bytecode; D3D
            // validates it before handing it to the hardware driver.
            match unsafe {
                result
                    .inner
                    .device
                    .CreatePixelShader(crate::fog::variant(crate::FogProgram::Dual).as_ptr())
            } {
                Ok(shader) => {
                    result.fog_shader = Some(shader);
                    for (slot, mode) in [
                        crate::FogProgram::Environment,
                        crate::FogProgram::Distance,
                        crate::FogProgram::Unfogged,
                        crate::FogProgram::FullyFogged,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        // SAFETY: SDK-compiled and checked ps_2_0 programs. A
                        // driver rejecting an optimization keeps the dual path.
                        match unsafe {
                            result
                                .inner
                                .device
                                .CreatePixelShader(crate::fog::variant(mode).as_ptr())
                        } {
                            Ok(shader) => result.fog_variants[slot] = Some(shader),
                            Err(error) => log::warn!(
                                "D3D9 fog {mode:?} unavailable ({error}); using dual curves"
                            ),
                        }
                    }
                    log::info!(
                        "D3D9 fog: ps_2_0 sphere/cylinder curves; fixed-function vertex processing"
                    );
                }
                Err(error) => log::warn!(
                    "D3D9 dual fog unavailable ({error}); fixed-function fog approximation"
                ),
            }
        } else {
            log::warn!("D3D9 dual fog unavailable in caps; fixed-function fog approximation");
        }
        if raw.PixelShaderVersion >= 0xffff_0200 {
            for fog in [false, true] {
                if fog && result.fog_shader.is_none() {
                    continue;
                }
                // SAFETY: SDK-compiled ps_2_0 bytecode with an END token.
                match unsafe {
                    result
                        .inner
                        .device
                        .CreatePixelShader(crate::fog::hurt_program(fog).as_ptr())
                } {
                    Ok(shader) => result.hurt_shaders[usize::from(fog)] = Some(shader),
                    Err(error) => log::warn!("D3D9 entity hurt shader unavailable ({error})"),
                }
            }
        }
        if !software_vertices
            && raw.VertexShaderVersion >= 0xfffe_0200
            && raw.MaxVertexShaderConst >= 9
            && result.fog_shader.is_some()
        {
            match TerrainPipeline::new(&result.inner.device) {
                Ok(pipeline) => {
                    result.terrain = Some(pipeline);
                    log::info!(
                        "D3D9 terrain: hardware vs_2_0, exact 20/28-byte vertices; full 32-byte fallback"
                    );
                }
                Err(error) => {
                    log::warn!("D3D9 compact vertex path unavailable ({error}); keeping FFP")
                }
            }
        }
        if result.terrain.is_none() {
            log::info!("D3D9 terrain: fixed-function 32-byte vertices; no vertex shader required");
        }
        #[cfg(feature = "submit-profile")]
        if crate::log_stats_enabled() {
            log::info!(
                "D3D9 submit_profile v=1 source_sha={} source_dirty={} compiler={} toolkit={} software_vertices={} terrain_ffp={} ps2={} hw_compact={} color_format={} depth_format={} index_bits=16 static_vb_pool={} transient_vb_pool=DEFAULT available_texture_bytes_advisory={}",
                option_env!("HORIZON_D3D9_BUILD_SHA").unwrap_or("unknown"),
                option_env!("HORIZON_D3D9_BUILD_DIRTY").unwrap_or("unknown"),
                option_env!("HORIZON_D3D9_BUILD_RUSTC").unwrap_or("unknown"),
                option_env!("HORIZON_D3D9_BUILD_CC").unwrap_or("unknown"),
                software_vertices,
                result.terrain.is_none(),
                result.fog_shader.is_some(),
                result.terrain.is_some(),
                present.BackBufferFormat.0,
                present.AutoDepthStencilFormat.0,
                if software_vertices {
                    "SYSTEMMEM"
                } else {
                    "DEFAULT"
                },
                result.caps.available_texture_bytes
            );
            log::info!(
                "D3D9 submit_schema com_order={} cache_requests_order=render,sampler,tss upload_bytes_order=immutable_vb,transient_vb,ib,texture_patch dip_quantiles=power_of_two_upper_bounds gpu_timing=not_instrumented",
                crate::submit_profile::API_NAMES
            );
        }
        // Test a real texture, VB and lightmap allocation before accepting the adapter.
        let _texture = result.texture(16, 16, 1, &[255; 1024])?;
        let _vertices = result.mesh(&[Vertex::zero(); 4])?;
        Ok(result)
    }
    /// Reuse a suspended renderer's device while replacing its resource pack.
    pub fn reuse(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            caps: self.caps.clone(),
            present: self.present,
            indices: self.indices.clone(),
            transient: None,
            transient_capacity: 0,
            transient_ring: Ring::default(),
            transient_serial: 0,
            screen: gui_device::Stream::default(),
            screen_active: false,
            dynamic_indices: None,
            index_ring: Ring::default(),
            using_dynamic_indices: false,
            reset_pending: false,
            surface_pending: true,
            lost: self.lost,
            scene: false,
            anisotropy: self.anisotropy,
            #[cfg(feature = "dev-diagnostics")]
            benchmark_slot: None,
            #[cfg(feature = "dev-diagnostics")]
            sampler_history_valid: false,
            cached_state: None,
            state_cache: StateCache::default(),
            cached_transform: None,
            fog_shader: self.fog_shader.clone(),
            fog_variants: self.fog_variants.clone(),
            hurt_shaders: self.hurt_shaders.clone(),
            shader_active: None,
            fog_constants: None,
            terrain: self.terrain.clone(),
            terrain_encoding: None,
            vertex_constants: [[0.0; 4]; 8],
            vertex_constants_dirty: true,
            draws: 0,
            fog_draws: [0; 5],
            binds: 0,
            uploads: 0,
            resets: self.resets,
            mesh_uploads: 0,
            stream_uploads: 0,
            stream_locks: 0,
            index_uploads: 0,
        }
    }
    pub fn set_anisotropy(&mut self, requested: u32) {
        let maximum =
            if self.caps.raw.TextureFilterCaps & D3DPTFILTERCAPS_MINFANISOTROPIC as u32 != 0 {
                self.caps.raw.MaxAnisotropy.clamp(1, 16)
            } else {
                1
            };
        let actual = requested.clamp(1, maximum);
        if self.anisotropy != actual {
            self.anisotropy = actual;
            self.cached_state = None;
            log::info!("D3D9 anisotropy requested={requested} effective={actual}");
        }
    }
    pub fn compact_vertices(&self) -> bool {
        self.terrain.is_some()
    }
    fn vertex_pipeline(&mut self, encoding: VertexEncoding) -> Result<()> {
        if encoding == VertexEncoding::Full {
            if self.terrain_encoding.take().is_some() || self.screen_active {
                // SAFETY: the canonical FVF matches every transient/full vertex.
                unsafe {
                    profile_call!(
                        self.inner,
                        SetVertexShader,
                        self.inner.device.SetVertexShader(None)
                    )?;
                    self.inner.device.SetFVF(FVF)?;
                }
                self.screen_active = false;
            }
            return Ok(());
        }
        let pipeline = self
            .terrain
            .as_ref()
            .ok_or("D3D9 compact vertices require the hardware VS2 path")?;
        if self.terrain_encoding != Some(encoding) {
            // SAFETY: encoding is private in Mesh/EncodedVertices and declarations match its stride.
            unsafe {
                profile_call!(
                    self.inner,
                    SetVertexShader,
                    self.inner.device.SetVertexShader(&pipeline.shader)
                )?;
                self.inner.device.SetVertexDeclaration(match encoding {
                    VertexEncoding::Compact => &pipeline.compact,
                    _ => &pipeline.wide,
                })?;
            }
            self.terrain_encoding = Some(encoding);
            self.screen_active = false;
            self.vertex_constants_dirty = true;
        }
        if self.vertex_constants_dirty {
            self.vertex_constants[7] = encoding.scales();
            // SAFETY: exactly eight contiguous float4 registers, the SDK program's input constants.
            unsafe {
                self.inner.device.SetVertexShaderConstantF(
                    0,
                    self.vertex_constants.as_ptr().cast(),
                    8,
                )?;
            }
            self.vertex_constants_dirty = false;
        }
        Ok(())
    }
    fn create_indices(&mut self) -> Result<()> {
        let quads = MAX_QUADS
            .min(self.caps.max_primitive_count / 2)
            .min((self.caps.raw.MaxVertexIndex.saturating_add(1)) / 4);
        if quads == 0 {
            return Err("D3D9 HAL has no indexed quad capacity".into());
        }
        let indices: Vec<u16> = (0..quads)
            .flat_map(|q| {
                let n = (q * 4) as u16;
                [n, n + 1, n + 2, n, n + 2, n + 3]
            })
            .collect();
        let mut buffer = None;
        // SAFETY: length matches the input bytes; the lock is copied into and unlocked here.
        unsafe {
            profile_call!(
                self.inner,
                CreateIndexBuffer,
                self.inner.device.CreateIndexBuffer(
                    (indices.len() * 2) as u32,
                    D3DUSAGE_WRITEONLY as u32,
                    D3DFMT_INDEX16,
                    D3DPOOL_MANAGED,
                    &mut buffer,
                    ptr::null_mut(),
                )
            )?;
            let buffer = buffer.ok_or("D3D9 index buffer missing")?;
            let mut memory = ptr::null_mut();
            profile_call!(
                self.inner,
                LockIndex,
                buffer.Lock(0, (indices.len() * 2) as u32, &mut memory, 0)
            )?;
            ptr::copy_nonoverlapping(
                indices.as_ptr().cast::<u8>(),
                memory.cast::<u8>(),
                indices.len() * 2,
            );
            profile_call!(self.inner, UnlockIndex, buffer.Unlock())?;
            self.indices = Some(buffer);
            profile_count!(self.inner, |c| c.upload_bytes[2] +=
                (indices.len() * 2) as u64);
        }
        Ok(())
    }
    pub fn max_vertices(&self) -> usize {
        (MAX_QUADS
            .min(self.caps.max_primitive_count / 2)
            .min(self.caps.raw.MaxVertexIndex.saturating_add(1) / 4)
            * 4) as usize
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.present.BackBufferWidth == width && self.present.BackBufferHeight == height {
            return;
        }
        self.present.BackBufferWidth = width;
        self.present.BackBufferHeight = height;
        self.surface_pending = true;
    }
    pub fn vsync(&mut self, on: bool) {
        let interval = if on {
            D3DPRESENT_INTERVAL_ONE as u32
        } else {
            D3DPRESENT_INTERVAL_IMMEDIATE as u32
        };
        if self.present.PresentationInterval != interval {
            self.present.PresentationInterval = interval;
            self.surface_pending = true;
        }
    }
    fn detach_presentation(&self) {
        // SAFETY: outside BeginScene; restore the implicit target before dropping
        // the additional target. The best-effort path also runs on a lost device.
        unsafe {
            let _ = self.inner.device.SetDepthStencilSurface(None);
            if let Ok(primary) = self
                .inner
                .device
                .GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO)
            {
                let _ = self.inner.device.SetRenderTarget(0, &primary);
            }
        }
        self.inner.presentation.borrow_mut().take();
        *self.inner.world.borrow_mut() = world_target::State::default();
    }
    fn create_presentation(&mut self) -> Result<()> {
        self.detach_presentation();
        let mut params = self.present;
        // SAFETY: all surfaces belong to this device, have matching dimensions,
        // formats and sample count, and are released together before a real Reset.
        unsafe {
            let mut chain = None;
            self.inner
                .device
                .CreateAdditionalSwapChain(&mut params, &mut chain)?;
            let chain = chain.ok_or("D3D9 additional swapchain missing")?;
            let mut depth = None;
            self.inner.device.CreateDepthStencilSurface(
                params.BackBufferWidth,
                params.BackBufferHeight,
                params.AutoDepthStencilFormat,
                D3DMULTISAMPLE_NONE,
                0,
                true,
                &mut depth,
                ptr::null_mut(),
            )?;
            let depth = depth.ok_or("D3D9 window depth surface missing")?;
            let target = chain.GetBackBuffer(0, D3DBACKBUFFER_TYPE_MONO)?;
            self.inner.device.SetRenderTarget(0, &target)?;
            self.inner.device.SetDepthStencilSurface(&depth)?;
            *self.inner.presentation.borrow_mut() = Some(Presentation {
                chain,
                _depth: depth,
                width: params.BackBufferWidth,
                height: params.BackBufferHeight,
                interval: params.PresentationInterval,
            });
        }
        self.surface_pending = false;
        log::info!(
            "D3D9 presentation {}x{} recreated; terrain buffers preserved",
            params.BackBufferWidth,
            params.BackBufferHeight
        );
        Ok(())
    }
    pub fn begin(&mut self, color: [f32; 3]) -> Result<bool> {
        self.begin_scaled(color, 1.0)
    }
    pub fn begin_scaled(&mut self, color: [f32; 3], scale: f32) -> Result<bool> {
        #[cfg(feature = "dev-diagnostics")]
        let sampler_history_valid = std::mem::take(&mut self.sampler_history_valid);
        if self.present.BackBufferWidth == 0 || self.present.BackBufferHeight == 0 {
            return Ok(false);
        }
        // SAFETY: this device is used only by its creating thread.
        let cooperative = unsafe { self.inner.device.TestCooperativeLevel() };
        match cooperative {
            Err(e) if e.code() == D3DERR_DEVICELOST => {
                self.lost = true;
                return Ok(false);
            }
            Err(e) if e.code() == D3DERR_DEVICENOTRESET => self.reset_pending = true,
            Err(e) => return Err(e.into()),
            Ok(()) => {}
        }
        #[cfg(feature = "dev-diagnostics")]
        let sampler_history_valid = sampler_history_valid && !self.lost && !self.reset_pending;
        if self.reset_pending {
            self.state_cache.stream.invalidate();
            self.detach_presentation();
            // Textures/indices are MANAGED. Terrain buffers have a CPU cache in the
            // caller; invalidate all DEFAULT handles, including reused-device owners.
            for handle in self.inner.default_meshes.borrow_mut().drain(..) {
                if let Some(buffer) = handle.upgrade() {
                    buffer.borrow_mut().take();
                }
            }
            self.transient = None;
            self.transient_capacity = 0;
            self.transient_ring.reset();
            self.transient_serial = 0;
            self.screen = gui_device::Stream::default();
            self.dynamic_indices = None;
            self.index_ring.reset();
            // SAFETY: no outstanding lock, scene, stateblock or backbuffer reference exists.
            unsafe {
                profile_call!(
                    self.inner,
                    SetStreamSource,
                    self.inner.device.SetStreamSource(0, None, 0, 0)
                )?;
                profile_call!(self.inner, SetIndices, self.inner.device.SetIndices(None))?;
                profile_call!(
                    self.inner,
                    SetTexture,
                    self.inner.device.SetTexture(0, None)
                )?;
                profile_call!(
                    self.inner,
                    SetTexture,
                    self.inner.device.SetTexture(1, None)
                )?;
                let mut primary = self.present;
                primary.BackBufferWidth = 1;
                primary.BackBufferHeight = 1;
                primary.PresentationInterval = D3DPRESENT_INTERVAL_IMMEDIATE as u32;
                match self.inner.device.Reset(&mut primary) {
                    Err(e) if e.code() == D3DERR_DEVICELOST => {
                        self.lost = true;
                        return Ok(false);
                    }
                    Err(e) => return Err(e.into()),
                    Ok(()) => {}
                }
            }
            log::info!(
                "D3D9 Reset {}x{} completed",
                self.present.BackBufferWidth,
                self.present.BackBufferHeight
            );
            self.reset_pending = false;
            self.lost = false;
            self.resets += 1;
            self.surface_pending = true;
        }
        let matches = self.inner.presentation.borrow().as_ref().is_some_and(|p| {
            p.width == self.present.BackBufferWidth
                && p.height == self.present.BackBufferHeight
                && p.interval == self.present.PresentationInterval
        });
        let recreate_presentation = self.surface_pending || !matches;
        if recreate_presentation && let Err(error) = self.create_presentation() {
            if is_device_lost(error.as_ref()) {
                self.lost = true;
                return Ok(false);
            }
            return Err(error);
        }
        self.using_dynamic_indices = false;
        self.mesh_uploads = 0;
        self.stream_uploads = 0;
        self.stream_locks = 0;
        self.transient_serial = 0;
        self.screen.serial = 0;
        self.screen_active = false;
        self.index_uploads = 0;
        self.draws = 0;
        self.fog_draws = [0; 5];
        self.binds = 0;
        self.terrain_encoding = None;
        self.vertex_constants_dirty = true;
        self.cached_state = None;
        if self.state_cache.textures[2] != usize::MAX {
            // SAFETY: clear the optional fixed-function lightmap binding on this device.
            unsafe {
                profile_call!(
                    self.inner,
                    SetTexture,
                    self.inner.device.SetTexture(2, None)
                )?;
            }
        }
        #[cfg(feature = "dev-diagnostics")]
        let keep_samplers = sampler_history_valid && !recreate_presentation;
        #[cfg(not(feature = "dev-diagnostics"))]
        let keep_samplers = false;
        if keep_samplers {
            self.state_cache.render.fill(None);
            self.state_cache.textures.fill(usize::MAX);
            self.state_cache.stream.invalidate();
            profile_count!(self.inner, |c| c.sampler_history_frames += 1);
        } else {
            self.state_cache = StateCache::default();
        }
        self.cached_transform = None;
        self.shader_active = None;
        self.fog_constants = None;
        // Select the world target before Clear, so scaling does not also clear a full-size window.
        self.select_world_target(scale)?;
        // SAFETY: state values are D3D9 constants; begin/finish pair on this thread.
        unsafe {
            profile_call!(
                self.inner,
                SetVertexShader,
                self.inner.device.SetVertexShader(None)
            )?;
            profile_call!(
                self.inner,
                SetPixelShader,
                self.inner.device.SetPixelShader(None)
            )?;
            self.inner.device.SetFVF(FVF)?;
            profile_call!(
                self.inner,
                SetIndices,
                self.inner.device.SetIndices(self.indices.as_ref())
            )?;
            profile_call!(
                self.inner,
                SetRenderState,
                self.inner.device.SetRenderState(D3DRS_LIGHTING, 0)
            )?;
            profile_call!(
                self.inner,
                SetRenderState,
                self.inner
                    .device
                    .SetRenderState(D3DRS_ZFUNC, D3DCMP_GREATEREQUAL.0 as u32)
            )?;
            profile_call!(
                self.inner,
                SetRenderState,
                self.inner
                    .device
                    .SetRenderState(D3DRS_ALPHAFUNC, D3DCMP_GREATER.0 as u32)
            )?;
            profile_call!(
                self.inner,
                SetRenderState,
                self.inner.device.SetRenderState(D3DRS_ALPHAREF, 25)
            )?;
            profile_call!(
                self.inner,
                SetRenderState,
                self.inner.device.SetRenderState(D3DRS_DITHERENABLE, 1)
            )?;
            // Clear obeys the previous frame's scissor too, including GUI item clips.
            profile_call!(
                self.inner,
                SetRenderState,
                self.inner.device.SetRenderState(D3DRS_SCISSORTESTENABLE, 0)
            )?;
            profile_call!(
                self.inner,
                Clear,
                self.inner.device.Clear(
                    0,
                    ptr::null(),
                    D3DCLEAR_TARGET as u32 | D3DCLEAR_ZBUFFER as u32,
                    argb([color[0], color[1], color[2], 1.0]),
                    0.0,
                    0,
                )
            )?;
            self.inner.device.BeginScene()?;
        }
        self.scene = true;
        Ok(true)
    }
    pub fn clear_depth(&mut self) -> Result<()> {
        self.clear_z(0.0)
    }
    pub fn clear_gui_depth(&mut self) -> Result<()> {
        self.clear_z(1.0)
    }
    fn clear_z(&mut self, far: f32) -> Result<()> {
        // Match a depth attachment clear, independently of the preceding draw's clip.
        self.state_cache
            .render(&self.inner, D3DRS_SCISSORTESTENABLE, 0)?;
        self.cached_state = None;
        // SAFETY: live depth surface; clear to the caller's depth convention's far value.
        unsafe {
            profile_call!(
                self.inner,
                Clear,
                self.inner
                    .device
                    .Clear(0, ptr::null(), D3DCLEAR_ZBUFFER as u32, 0, far, 0)
            )?;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<bool> {
        #[cfg(feature = "dev-diagnostics")]
        let remember_samplers =
            self.scene && self.benchmark_active(crate::benchmark::Trial::SamplerReuse);
        if self.scene {
            // SAFETY: closes the matching BeginScene, including after a drawing error.
            let ended = unsafe { self.inner.device.EndScene() };
            self.scene = false;
            match ended {
                Err(e) if e.code() == D3DERR_DEVICELOST || e.code() == D3DERR_DEVICENOTRESET => {
                    self.lost = true;
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
                Ok(()) => {}
            }
        }
        // SAFETY: null rectangles present the complete windowed swapchain.
        match unsafe {
            self.inner
                .presentation
                .borrow()
                .as_ref()
                .ok_or("D3D9 presentation surface missing")?
                .chain
                .Present(ptr::null(), ptr::null(), HWND::default(), ptr::null(), 0)
        } {
            Err(e) if e.code() == D3DERR_DEVICELOST || e.code() == D3DERR_DEVICENOTRESET => {
                self.lost = true;
                Ok(false)
            }
            Err(e) => Err(e.into()),
            Ok(()) => {
                #[cfg(feature = "dev-diagnostics")]
                {
                    self.sampler_history_valid = remember_samplers;
                }
                Ok(true)
            }
        }
    }
    pub fn cancel_frame(&mut self) {
        #[cfg(feature = "dev-diagnostics")]
        {
            self.sampler_history_valid = false;
        }
        if self.scene {
            // SAFETY: best-effort matching EndScene on an error path; never called twice.
            let _ = unsafe { self.inner.device.EndScene() };
            self.scene = false;
        }
        if self.inner.world.borrow().active {
            let _ = self.restore_main_target();
            self.inner.world.borrow_mut().active = false;
        }
    }
    pub fn mark_lost(&mut self) {
        #[cfg(feature = "dev-diagnostics")]
        {
            self.sampler_history_valid = false;
        }
        self.lost = true;
    }
    pub fn is_lost(&self) -> bool {
        self.lost
    }
    pub fn lost(&self) -> bool {
        self.lost
    }
    pub fn transform(
        &mut self,
        world: [f32; 16],
        mut view: [f32; 16],
        mut projection: [f32; 16],
    ) -> Result<()> {
        profile_count!(self.inner, |c| c.transform_requests += 1);
        let original_view = view;
        let original_projection = projection;
        // A pair of Z reflections preserves every clip-space coordinate while
        // making D3D fixed-function camera-space Z positive for linear vertex fog.
        for index in [2, 6, 10, 14] {
            view[index] = -view[index];
        }
        for index in [8, 9, 10, 11] {
            projection[index] = -projection[index];
        }
        let previous = self.cached_transform;
        if self.terrain.is_some() && previous != Some([world, view, projection]) {
            self.vertex_constants =
                crate::terrain::constants(&world, &original_view, &original_projection);
            self.vertex_constants_dirty = true;
        }
        // SAFETY: Matrix4x4 is repr(C), sixteen f32 values in row order; glam's
        // column storage supplies the transpose for D3D's row-vector convention.
        unsafe {
            if previous.is_none_or(|p| p[0] != world) {
                profile_call!(
                    self.inner,
                    SetTransform,
                    self.inner.device.SetTransform(
                        D3DTS_WORLD,
                        &std::mem::transmute::<[f32; 16], Matrix4x4>(world),
                    )
                )?;
            }
            if previous.is_none_or(|p| p[1] != view) {
                profile_call!(
                    self.inner,
                    SetTransform,
                    self.inner.device.SetTransform(
                        D3DTS_VIEW,
                        &std::mem::transmute::<[f32; 16], Matrix4x4>(view),
                    )
                )?;
                if self.fog_shader.is_some() {
                    let inverse = glam::Mat4::from_cols_array(&view).inverse().to_cols_array();
                    profile_call!(
                        self.inner,
                        SetTransform,
                        self.inner.device.SetTransform(
                            D3DTS_TEXTURE2,
                            &std::mem::transmute::<[f32; 16], Matrix4x4>(inverse),
                        )
                    )?;
                }
            }
            if previous.is_none_or(|p| p[2] != projection) {
                profile_call!(
                    self.inner,
                    SetTransform,
                    self.inner.device.SetTransform(
                        D3DTS_PROJECTION,
                        &std::mem::transmute::<[f32; 16], Matrix4x4>(projection),
                    )
                )?;
            }
        }
        self.cached_transform = Some([world, view, projection]);
        Ok(())
    }
    pub fn texture(
        &mut self,
        width: u32,
        height: u32,
        levels: u32,
        rgba: &[u8],
    ) -> Result<Texture> {
        self.texture_with_pixels(width, height, levels, Some(rgba))
    }

    /// Allocate a single-level managed texture filled with transparent black.
    pub fn zeroed_texture(&mut self, width: u32, height: u32) -> Result<Texture> {
        self.texture_with_pixels(width, height, 1, None)
    }

    fn texture_with_pixels(
        &mut self,
        width: u32,
        height: u32,
        levels: u32,
        rgba: Option<&[u8]>,
    ) -> Result<Texture> {
        if width == 0
            || height == 0
            || width > self.caps.max_texture
            || height > self.caps.max_texture
            || rgba.is_some_and(|pixels| pixels.len() != width as usize * height as usize * 4)
        {
            return Err(format!(
                "D3D9 texture: invalid {}x{}, {} bytes (caps {})",
                width,
                height,
                rgba.map_or(0, <[u8]>::len),
                self.caps.max_texture
            )
            .into());
        }
        let mut raw = None;
        // SAFETY: output is initialized and kept owned; no shared resource handles.
        unsafe {
            self.inner.device.CreateTexture(
                width,
                height,
                levels.max(1),
                0,
                D3DFMT_A8R8G8B8,
                D3DPOOL_MANAGED,
                &mut raw,
                ptr::null_mut(),
            )?;
        }
        let raw = raw.ok_or("D3D9 texture missing")?;
        let base = raw.cast::<IDirect3DBaseTexture9>()?;
        let texture = Texture {
            mip_backup: None,
            raw,
            base,
            owner: self.inner.clone(),
            width,
            height,
            levels: levels.max(1),
            bytes: (0..levels.max(1))
                .map(|l| u64::from((width >> l).max(1)) * u64::from((height >> l).max(1)) * 4)
                .sum(),
        };
        self.write_texture(&texture, 0, 0, 0, width, height, rgba)?;
        Ok(texture)
    }

    pub fn patch(
        &mut self,
        texture: &Texture,
        level: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        self.write_texture(texture, level, x, y, width, height, Some(rgba))
    }

    fn write_texture(
        &mut self,
        texture: &Texture,
        level: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: Option<&[u8]>,
    ) -> Result<()> {
        if !Rc::ptr_eq(&texture.owner, &self.inner)
            || level >= texture.levels
            || width == 0
            || height == 0
            || x.checked_add(width)
                .is_none_or(|v| v > (texture.width >> level).max(1))
            || y.checked_add(height)
                .is_none_or(|v| v > (texture.height >> level).max(1))
            || rgba.is_some_and(|pixels| pixels.len() != width as usize * height as usize * 4)
        {
            return Err("D3D9 texture patch bounds/device mismatch".into());
        }
        let rect = RECT {
            left: x as i32,
            top: y as i32,
            right: (x + width) as i32,
            bottom: (y + height) as i32,
        };
        let row_bytes = width as usize * 4;
        let mut locked = D3DLOCKED_RECT::default();
        // SAFETY: validated input length and rect. Each write stays within
        // the texel bytes of the locked rows, using the runtime's pitch.
        unsafe {
            profile_call!(
                self.inner,
                LockTexture,
                texture.raw.LockRect(level, &mut locked, &rect, 0)
            )?;
            if locked.pBits.is_null() || locked.Pitch < row_bytes as i32 {
                profile_call!(self.inner, UnlockTexture, texture.raw.UnlockRect(level))?;
                return Err("D3D9 invalid texture lock pitch".into());
            }
            let dest = locked.pBits.cast::<u8>();
            let pitch = locked.Pitch as usize;
            if let Some(rgba) = rgba {
                for row in 0..height as usize {
                    let dest = std::slice::from_raw_parts_mut(dest.add(row * pitch), row_bytes);
                    let source = &rgba[row * row_bytes..(row + 1) * row_bytes];
                    for (d, s) in dest
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(source.as_chunks::<4>().0)
                    {
                        d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
                    }
                }
            } else {
                zero_texture_rows(dest, row_bytes, pitch, height as usize);
            }
            profile_call!(self.inner, UnlockTexture, texture.raw.UnlockRect(level))?;
        }
        let bytes = row_bytes as u64 * u64::from(height);
        self.uploads += bytes;
        profile_count!(self.inner, |c| c.upload_bytes[3] += bytes);
        Ok(())
    }
    pub fn mesh(&mut self, vertices: &[Vertex]) -> Result<Mesh> {
        if vertices.is_empty() || vertices.len() > self.max_vertices() || vertices.len() % 4 != 0 {
            return Err("D3D9 mesh outside quad/caps limits".into());
        }
        if self.compact_vertices() {
            return self.mesh_encoded(&EncodedVertices::new(vertices, true));
        }
        self.mesh_bytes(
            bytemuck::cast_slice(vertices),
            vertices.len(),
            VertexEncoding::Full,
        )
    }
    pub fn mesh_encoded(&mut self, vertices: &EncodedVertices) -> Result<Mesh> {
        self.mesh_encoded_range(vertices, 0..vertices.len())
    }
    pub fn mesh_encoded_range(
        &mut self,
        vertices: &EncodedVertices,
        range: std::ops::Range<usize>,
    ) -> Result<Mesh> {
        if vertices.encoding() != VertexEncoding::Full && !self.compact_vertices() {
            return Err("D3D9 compact upload requires supported hardware vertex processing".into());
        }
        if range.start > range.end
            || range.end > vertices.len()
            || range.start % 4 != 0
            || range.end % 4 != 0
        {
            return Err("D3D9 encoded upload range is outside complete quads".into());
        }
        let stride = vertices.encoding().stride();
        self.mesh_bytes(
            &vertices.bytes()[range.start * stride..range.end * stride],
            range.len(),
            vertices.encoding(),
        )
    }
    fn mesh_bytes(&mut self, bytes: &[u8], count: usize, encoding: VertexEncoding) -> Result<Mesh> {
        if count == 0 || count > self.max_vertices() || count % 4 != 0 {
            return Err("D3D9 mesh must contain complete quads within the caps/index limit".into());
        }
        let mut raw = None;
        // SAFETY: bounded allocation and matching lock length; the slice cannot escape the lock.
        unsafe {
            profile_call!(
                self.inner,
                CreateVertexBuffer,
                self.inner.device.CreateVertexBuffer(
                    bytes.len() as u32,
                    D3DUSAGE_WRITEONLY as u32
                        | if self.caps.software_vertices {
                            D3DUSAGE_SOFTWAREPROCESSING as u32
                        } else {
                            0
                        },
                    if encoding == VertexEncoding::Full {
                        FVF
                    } else {
                        0
                    },
                    if self.caps.software_vertices {
                        D3DPOOL_SYSTEMMEM
                    } else {
                        D3DPOOL_DEFAULT
                    },
                    &mut raw,
                    ptr::null_mut(),
                )
            )?;
            let raw = raw.ok_or("D3D9 vertex buffer missing")?;
            let mut memory = ptr::null_mut();
            profile_call!(
                self.inner,
                LockVertex,
                raw.Lock(0, bytes.len() as u32, &mut memory, 0)
            )?;
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            profile_call!(self.inner, UnlockVertex, raw.Unlock())?;
            self.uploads += bytes.len() as u64;
            self.mesh_uploads += bytes.len() as u64;
            profile_count!(self.inner, |c| c.upload_bytes[0] += bytes.len() as u64);
            let raw = Rc::new(RefCell::new(Some(raw)));
            let mut handles = self.inner.default_meshes.borrow_mut();
            if handles.len() >= 4096 && handles.len().is_power_of_two() {
                handles.retain(|h| h.strong_count() != 0);
            }
            handles.push(Rc::downgrade(&raw));
            Ok(Mesh {
                raw,
                owner: self.inner.clone(),
                vertices: count as u32,
                filled: count as u32,
                encoding,
            })
        }
    }
    fn shared_indices(&mut self) -> Result<()> {
        if self.using_dynamic_indices {
            // SAFETY: index buffer is owned by this device and survives Reset.
            unsafe {
                profile_call!(
                    self.inner,
                    SetIndices,
                    self.inner.device.SetIndices(self.indices.as_ref())
                )?;
            }
            self.using_dynamic_indices = false;
        }
        Ok(())
    }
    pub fn draw_batch(&mut self, mesh: &Mesh, batch: &IndexBatch) -> Result<()> {
        let bytes = bytemuck::cast_slice::<_, u8>(batch.indices());
        if bytes.is_empty() {
            return Ok(());
        }
        const CAPACITY: usize = 1 << 20;
        if !Rc::ptr_eq(&mesh.owner, &self.inner)
            || batch.end_vertex() > mesh.filled
            || batch.indices().len() / 3 > self.caps.max_primitive_count as usize
            || bytes.len() > CAPACITY
        {
            return Err("D3D9 terrain index batch exceeds its buffer/caps".into());
        }
        self.vertex_pipeline(mesh.encoding)?;
        if self.dynamic_indices.is_none() {
            let mut raw = None;
            // SAFETY: bounded DEFAULT index ring, released/unbound before Reset.
            unsafe {
                profile_call!(
                    self.inner,
                    CreateIndexBuffer,
                    self.inner.device.CreateIndexBuffer(
                        CAPACITY as u32,
                        (D3DUSAGE_DYNAMIC | D3DUSAGE_WRITEONLY) as u32
                            | if self.caps.software_vertices {
                                D3DUSAGE_SOFTWAREPROCESSING as u32
                            } else {
                                0
                            },
                        D3DFMT_INDEX16,
                        D3DPOOL_DEFAULT,
                        &mut raw,
                        ptr::null_mut(),
                    )
                )?;
            }
            self.dynamic_indices = raw;
            self.index_ring.reset();
        }
        let indices = self
            .dynamic_indices
            .as_ref()
            .ok_or("D3D9 terrain index ring missing")?;
        let (offset, discard) = self
            .index_ring
            .reserve(bytes.len(), CAPACITY)
            .ok_or("D3D9 index ring overflow")?;
        let raw = mesh.raw.borrow();
        let raw = raw.as_ref().ok_or("D3D9 arena requires re-upload")?;
        // SAFETY: IndexBatch validates quad indices, range checked against filled
        // vertices above; NOOVERWRITE only appends to storage not used since DISCARD.
        unsafe {
            let mut memory = ptr::null_mut();
            profile_call!(
                self.inner,
                LockIndex,
                indices.Lock(
                    offset as u32,
                    bytes.len() as u32,
                    &mut memory,
                    if discard {
                        D3DLOCK_DISCARD
                    } else {
                        D3DLOCK_NOOVERWRITE
                    } as u32,
                )
            )?;
            if memory.is_null() {
                profile_call!(self.inner, UnlockIndex, indices.Unlock())?;
                return Err("D3D9 empty index lock".into());
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            profile_call!(self.inner, UnlockIndex, indices.Unlock())?;
            profile_count!(self.inner, |c| c.upload_bytes[2] += bytes.len() as u64);
            profile_call!(
                self.inner,
                SetIndices,
                self.inner.device.SetIndices(indices)
            )?;
            self.using_dynamic_indices = true;
            self.state_cache.bind_stream(
                &self.inner,
                raw,
                mesh.stride() as u32,
                crate::development_build() && self.caps.software_vertices,
            )?;
            profile_call!(
                self.inner,
                DrawIndexedPrimitive,
                self.inner.device.DrawIndexedPrimitive(
                    D3DPT_TRIANGLELIST,
                    0,
                    batch.start_vertex(),
                    batch.end_vertex() - batch.start_vertex(),
                    offset as u32 / 2,
                    batch.indices().len() as u32 / 3,
                )
            )?;
        }
        self.uploads += bytes.len() as u64;
        self.index_uploads += bytes.len() as u64;
        profile_count!(self.inner, |c| {
            c.draw(
                true,
                batch.end_vertex() - batch.start_vertex(),
                batch.indices().len() as u32 / 3,
                0,
                false,
            );
        });
        self.record_draw();
        Ok(())
    }
    pub fn cache_indices(&mut self, batch: &IndexBatch) -> Result<Indices> {
        if batch.indices().is_empty()
            || !batch.groups_complete()
            || batch.indices().len() > u32::MAX as usize / 2
        {
            return Err("D3D9 cached index batch exceeds caps".into());
        }
        let windows = if batch.windows().is_empty() {
            vec![crate::geometry::IndexWindow {
                first: 0,
                count: batch.indices().len() as u32,
                start: batch.start_vertex(),
                end: batch.end_vertex(),
            }]
        } else {
            batch.windows().to_vec()
        };
        if windows
            .iter()
            .any(|w| w.count / 3 > self.caps.max_primitive_count)
        {
            return Err("D3D9 cached draw group exceeds primitive caps".into());
        }
        let bytes = bytemuck::cast_slice::<_, u8>(batch.indices());
        let mut raw = None;
        // SAFETY: complete validated quad indices and exact lock/copy sizes.
        unsafe {
            profile_call!(
                self.inner,
                CreateIndexBuffer,
                self.inner.device.CreateIndexBuffer(
                    bytes.len() as u32,
                    D3DUSAGE_WRITEONLY as u32
                        | if self.caps.software_vertices {
                            D3DUSAGE_SOFTWAREPROCESSING as u32
                        } else {
                            0
                        },
                    D3DFMT_INDEX16,
                    if self.caps.software_vertices {
                        D3DPOOL_SYSTEMMEM
                    } else {
                        D3DPOOL_MANAGED
                    },
                    &mut raw,
                    ptr::null_mut(),
                )
            )?;
            let raw = raw.ok_or("D3D9 cached index buffer missing")?;
            let mut memory = ptr::null_mut();
            profile_call!(
                self.inner,
                LockIndex,
                raw.Lock(0, bytes.len() as u32, &mut memory, 0)
            )?;
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            profile_call!(self.inner, UnlockIndex, raw.Unlock())?;
            self.uploads += bytes.len() as u64;
            self.index_uploads += bytes.len() as u64;
            profile_count!(self.inner, |c| c.upload_bytes[2] += bytes.len() as u64);
            Ok(Indices {
                raw,
                owner: self.inner.clone(),
                windows,
                count: batch.indices().len() as u32,
            })
        }
    }
    pub fn draw_cached(&mut self, mesh: &Mesh, indices: &Indices) -> Result<()> {
        if indices.windows.len() != 1 {
            return Err("D3D9 multi-group indices require an explicit draw group".into());
        }
        self.draw_cached_group(mesh, indices, 0)
    }
    pub fn draw_cached_group(&mut self, mesh: &Mesh, indices: &Indices, group: u32) -> Result<()> {
        let window = indices
            .windows
            .get(group as usize)
            .ok_or("D3D9 missing cached index group")?;
        self.draw_index_windows(mesh, indices, std::slice::from_ref(window))
    }
    pub fn draw_dense_solid(
        &mut self,
        mesh: &Mesh,
        indices: &Indices,
        _savings: crate::IndexSavings,
    ) -> Result<()> {
        self.draw_index_windows(mesh, indices, &indices.windows)?;
        profile_count!(self.inner, |c| {
            c.solid_draws_saved += u64::from(_savings.draws);
            c.solid_extra_vertices += u64::from(_savings.extra_vertices);
        });
        Ok(())
    }
    fn draw_index_windows(
        &mut self,
        mesh: &Mesh,
        indices: &Indices,
        windows: &[crate::geometry::IndexWindow],
    ) -> Result<()> {
        if !Rc::ptr_eq(&mesh.owner, &self.inner)
            || !Rc::ptr_eq(&indices.owner, &self.inner)
            || windows.iter().any(|window| window.end > mesh.filled)
        {
            return Err("D3D9 cached draw bounds/device mismatch".into());
        }
        self.vertex_pipeline(mesh.encoding)?;
        let raw = mesh.raw.borrow();
        let raw = raw.as_ref().ok_or("D3D9 arena requires re-upload")?;
        // SAFETY: index/vertex ranges were validated against this device and mesh.
        unsafe {
            profile_call!(
                self.inner,
                SetIndices,
                self.inner.device.SetIndices(&indices.raw)
            )?;
            self.using_dynamic_indices = true;
            self.state_cache.bind_stream(
                &self.inner,
                raw,
                mesh.stride() as u32,
                crate::development_build() && self.caps.software_vertices,
            )?;
        }
        for window in windows {
            let vertices = window.end - window.start;
            let triangles = window.count / 3;
            // SAFETY: the validated windows refer to the bound mesh and indices.
            unsafe {
                profile_call!(
                    self.inner,
                    DrawIndexedPrimitive,
                    self.inner.device.DrawIndexedPrimitive(
                        D3DPT_TRIANGLELIST,
                        0,
                        window.start,
                        vertices,
                        window.first,
                        triangles,
                    )
                )?;
            }
            profile_count!(self.inner, |c| c.draw(true, vertices, triangles, 0, false));
            self.record_draw();
        }
        Ok(())
    }
    pub fn state(
        &mut self,
        state: DrawState,
        texture: &Texture,
        lightmap: Option<&Texture>,
    ) -> Result<()> {
        profile_count!(self.inner, |c| c.state_requests += 1);
        if !Rc::ptr_eq(&texture.owner, &self.inner)
            || lightmap.is_some_and(|t| !Rc::ptr_eq(&t.owner, &self.inner))
        {
            return Err("D3D9 texture belongs to a different device".into());
        }
        let key = (
            state,
            texture.raw.as_raw() as usize,
            lightmap.map_or(0, |t| t.raw.as_raw() as usize),
        );
        if self.cached_state == Some(key) {
            return Ok(());
        }
        #[cfg(feature = "dev-diagnostics")]
        let previous = self.cached_state;
        // Publish the whole key only after the native calls have succeeded.
        self.cached_state = None;
        if self.state_cache.textures[0] != key.1 {
            // SAFETY: the base texture was checked against the native owner above.
            unsafe {
                profile_call!(
                    self.inner,
                    SetTexture,
                    self.inner.device.SetTexture(0, &texture.base)
                )?;
            }
            self.state_cache.textures[0] = key.1;
            self.binds += 1;
        }
        // All remaining settings for an ordinary unclipped draw depend on
        // DrawState, the auxiliary texture, and the current device settings.
        #[cfg(feature = "dev-diagnostics")]
        if self.benchmark_active(crate::benchmark::Trial::BaseTextureReuse)
            && state.scissor.is_none()
            && state.hurt_overlay.is_none()
            && state.glint == 0
            && previous.is_some_and(|(old, _, auxiliary)| old == state && auxiliary == key.2)
        {
            self.cached_state = Some(key);
            profile_count!(self.inner, |c| c.base_texture_only += 1);
            return Ok(());
        }
        let hurt = state
            .hurt_overlay
            .filter(|_| state.lightmap && state.glint == 0);
        let hurt_fog = state.fog.is_some();
        let shader_hurt = hurt.is_some() && self.hurt_shaders[usize::from(hurt_fog)].is_some();
        let ffp_hurt = hurt.filter(|_| {
            !shader_hurt
                && self.caps.raw.MaxTextureBlendStages >= 3
                && self.caps.raw.TextureOpCaps & D3DTEXOPCAPS_BLENDFACTORALPHA as u32 != 0
        });
        // SAFETY: validated resource ownership and documented D3D9 state constants.
        unsafe {
            let d = &self.inner.device;
            let cache = &mut self.state_cache;
            cache.stage(
                &self.inner,
                0,
                D3DTSS_COLOROP,
                if state.glint != 0 {
                    D3DTOP_SELECTARG2
                } else {
                    D3DTOP_MODULATE
                }
                .0 as u32,
            )?;
            cache.stage(&self.inner, 0, D3DTSS_COLORARG1, D3DTA_TEXTURE)?;
            cache.stage(&self.inner, 0, D3DTSS_COLORARG2, D3DTA_DIFFUSE)?;
            cache.stage(&self.inner, 0, D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32)?;
            cache.stage(&self.inner, 0, D3DTSS_ALPHAARG1, D3DTA_TEXTURE)?;
            cache.stage(&self.inner, 0, D3DTSS_ALPHAARG2, D3DTA_DIFFUSE)?;
            cache.stage(&self.inner, 0, D3DTSS_TEXCOORDINDEX, 0)?;
            let glint_square = state.glint != 0 && self.caps.raw.MaxTextureBlendStages >= 3;
            let lm = if state.lightmap || state.glint != 0 {
                lightmap
            } else {
                None
            };
            let light_texture = lm.filter(|_| ffp_hurt.is_none()).map(|t| &t.base);
            let light_id = light_texture.map_or(0, |t| t.as_raw() as usize);
            if cache.textures[1] != light_id {
                profile_call!(self.inner, SetTexture, d.SetTexture(1, light_texture))?;
                cache.textures[1] = light_id;
                self.binds += 1;
            }
            cache.stage(
                &self.inner,
                1,
                D3DTSS_COLOROP,
                if ffp_hurt.is_some() {
                    D3DTOP_BLENDFACTORALPHA.0
                } else if lm.is_some() {
                    D3DTOP_MODULATE.0
                } else {
                    D3DTOP_DISABLE.0
                } as u32,
            )?;
            cache.stage(
                &self.inner,
                1,
                D3DTSS_COLORARG1,
                if ffp_hurt.is_some() {
                    D3DTA_CURRENT
                } else {
                    D3DTA_TEXTURE
                },
            )?;
            cache.stage(
                &self.inner,
                1,
                D3DTSS_COLORARG2,
                if ffp_hurt.is_some() {
                    D3DTA_TFACTOR
                } else if glint_square {
                    D3DTA_TEXTURE
                } else {
                    D3DTA_CURRENT
                },
            )?;
            cache.stage(
                &self.inner,
                2,
                D3DTSS_COLOROP,
                if glint_square || ffp_hurt.is_some() {
                    D3DTOP_MODULATE
                } else {
                    D3DTOP_DISABLE
                }
                .0 as u32,
            )?;
            if glint_square {
                cache.stage(&self.inner, 2, D3DTSS_COLORARG1, D3DTA_CURRENT)?;
                cache.stage(&self.inner, 2, D3DTSS_COLORARG2, D3DTA_DIFFUSE)?;
                cache.stage(&self.inner, 2, D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32)?;
                cache.stage(&self.inner, 2, D3DTSS_ALPHAARG1, D3DTA_CURRENT)?;
            }
            if let Some(hurt) = ffp_hurt {
                cache.render(&self.inner, D3DRS_TEXTUREFACTOR, rgba_to_argb(hurt))?;
                let texture = lm.map(|t| &t.base);
                let id = texture.map_or(0, |t| t.as_raw() as usize);
                if cache.textures[2] != id {
                    profile_call!(self.inner, SetTexture, d.SetTexture(2, texture))?;
                    cache.textures[2] = id;
                    self.binds += 1;
                }
                cache.stage(&self.inner, 2, D3DTSS_COLORARG1, D3DTA_CURRENT)?;
                cache.stage(&self.inner, 2, D3DTSS_COLORARG2, D3DTA_TEXTURE)?;
                cache.stage(&self.inner, 2, D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32)?;
                cache.stage(&self.inner, 2, D3DTSS_ALPHAARG1, D3DTA_CURRENT)?;
                cache.stage(&self.inner, 2, D3DTSS_TEXCOORDINDEX, 1)?;
                cache.stage(
                    &self.inner,
                    2,
                    D3DTSS_TEXTURETRANSFORMFLAGS,
                    D3DTTFF_DISABLE.0 as u32,
                )?;
                cache.sampler(&self.inner, 2, D3DSAMP_MINFILTER, D3DTEXF_LINEAR.0 as u32)?;
                cache.sampler(&self.inner, 2, D3DSAMP_MAGFILTER, D3DTEXF_LINEAR.0 as u32)?;
                cache.sampler(&self.inner, 2, D3DSAMP_MIPFILTER, D3DTEXF_NONE.0 as u32)?;
                cache.sampler(&self.inner, 2, D3DSAMP_ADDRESSU, D3DTADDRESS_CLAMP.0 as u32)?;
                cache.sampler(&self.inner, 2, D3DSAMP_ADDRESSV, D3DTADDRESS_CLAMP.0 as u32)?;
            } else if cache.textures[2] != usize::MAX {
                profile_call!(self.inner, SetTexture, d.SetTexture(2, None))?;
                cache.textures[2] = usize::MAX;
            }
            cache.stage(&self.inner, 1, D3DTSS_ALPHAOP, D3DTOP_SELECTARG2.0 as u32)?;
            cache.stage(&self.inner, 1, D3DTSS_ALPHAARG2, D3DTA_CURRENT)?;
            cache.stage(&self.inner, 1, D3DTSS_TEXCOORDINDEX, 1)?;
            let (src, dst) = match state.blend {
                Blend::Opaque | Blend::Alpha => (D3DBLEND_SRCALPHA, D3DBLEND_INVSRCALPHA),
                Blend::Additive => (D3DBLEND_SRCALPHA, D3DBLEND_ONE),
                Blend::Invert => (D3DBLEND_INVDESTCOLOR, D3DBLEND_INVSRCCOLOR),
                Blend::Multiply => (D3DBLEND_ZERO, D3DBLEND_INVSRCCOLOR),
                Blend::Cracks => (D3DBLEND_DESTCOLOR, D3DBLEND_SRCCOLOR),
            };
            cache.render(
                &self.inner,
                D3DRS_ALPHABLENDENABLE,
                u32::from(state.blend != Blend::Opaque),
            )?;
            cache.render(&self.inner, D3DRS_SRCBLEND, src.0 as u32)?;
            cache.render(&self.inner, D3DRS_DESTBLEND, dst.0 as u32)?;
            cache.render(&self.inner, D3DRS_ZENABLE, u32::from(state.depth_test))?;
            cache.render(
                &self.inner,
                D3DRS_ZWRITEENABLE,
                u32::from(state.depth_write),
            )?;
            cache.render(
                &self.inner,
                D3DRS_ZFUNC,
                if state.equal_depth {
                    D3DCMP_EQUAL
                } else if state.reverse_z {
                    D3DCMP_GREATEREQUAL
                } else {
                    D3DCMP_LESSEQUAL
                }
                .0 as u32,
            )?;
            cache.render(
                &self.inner,
                D3DRS_ALPHATESTENABLE,
                u32::from(state.alpha_test),
            )?;
            cache.render(&self.inner, D3DRS_ALPHAREF, u32::from(state.alpha_ref))?;
            cache.render(
                &self.inner,
                D3DRS_CULLMODE,
                if state.cull {
                    D3DCULL_CW.0
                } else {
                    D3DCULL_NONE.0
                } as u32,
            )?;
            cache.render(
                &self.inner,
                D3DRS_SCISSORTESTENABLE,
                u32::from(state.scissor.is_some()),
            )?;
            if let Some([x, y, w, h]) = state.scissor {
                d.SetScissorRect(&RECT {
                    left: x as i32,
                    top: y as i32,
                    right: (x + w) as i32,
                    bottom: (y + h) as i32,
                })?;
            }
            let mode = if shader_hurt {
                Some(PixelProgram::Hurt { fog: hurt_fog })
            } else if hurt_fog && state.lightmap && self.fog_shader.is_some() && ffp_hurt.is_none()
            {
                let at = state.fog_program as usize;
                Some(PixelProgram::Fog(
                    if at > 0 && self.fog_variants[at - 1].is_some() {
                        state.fog_program
                    } else {
                        crate::FogProgram::Dual
                    },
                ))
            } else {
                None
            };
            if mode != self.shader_active {
                profile_call!(
                    self.inner,
                    SetPixelShader,
                    d.SetPixelShader(match mode {
                        Some(PixelProgram::Fog(crate::FogProgram::Dual)) =>
                            self.fog_shader.as_ref(),
                        Some(PixelProgram::Fog(mode)) =>
                            self.fog_variants[mode as usize - 1].as_ref(),
                        Some(PixelProgram::Hurt { fog }) =>
                            self.hurt_shaders[usize::from(fog)].as_ref(),
                        None => None,
                    })
                )?;
                self.shader_active = mode;
            }
            let fog_mode = mode.and_then(PixelProgram::fog);
            if fog_mode.is_some_and(crate::FogProgram::position) {
                cache.stage(
                    &self.inner,
                    2,
                    D3DTSS_TEXCOORDINDEX,
                    D3DTSS_TCI_CAMERASPACEPOSITION | 2,
                )?;
                cache.stage(
                    &self.inner,
                    2,
                    D3DTSS_TEXTURETRANSFORMFLAGS,
                    D3DTTFF_COUNT3.0 as u32,
                )?;
            } else if self.fog_shader.is_some() && ffp_hurt.is_none() {
                cache.stage(
                    &self.inner,
                    2,
                    D3DTSS_TEXTURETRANSFORMFLAGS,
                    D3DTTFF_DISABLE.0 as u32,
                )?;
                cache.stage(&self.inner, 2, D3DTSS_TEXCOORDINDEX, 2)?;
            }
            let shader_fog = fog_mode.is_some();
            if shader_fog && self.fog_constants != state.fog {
                let constants = state.fog.unwrap().constants();
                d.SetPixelShaderConstantF(0, constants.as_ptr().cast(), 3)?;
                self.fog_constants = state.fog;
            }
            if shader_hurt {
                let texel = hurt
                    .unwrap()
                    .to_le_bytes()
                    .map(|value| f32::from(value) / 255.0);
                d.SetPixelShaderConstantF(3, texel.as_ptr(), 1)?;
            }
            cache.render(
                &self.inner,
                D3DRS_FOGENABLE,
                u32::from(state.fog.is_some() && !shader_fog),
            )?;
            if let Some(fog) = state.fog
                && !shader_fog
            {
                let color = fog.color;
                let (start, end) = crate::fog_interval(
                    (fog.environment[0], fog.environment[1]),
                    (fog.distance[0], fog.distance[1]),
                );
                cache.render(
                    &self.inner,
                    D3DRS_FOGCOLOR,
                    argb([color[0], color[1], color[2], 1.0]),
                )?;
                cache.render(&self.inner, D3DRS_FOGTABLEMODE, D3DFOG_NONE.0 as u32)?;
                cache.render(&self.inner, D3DRS_FOGVERTEXMODE, D3DFOG_LINEAR.0 as u32)?;
                cache.render(
                    &self.inner,
                    D3DRS_RANGEFOGENABLE,
                    u32::from(self.caps.raw.RasterCaps & D3DPRASTERCAPS_FOGRANGE as u32 != 0),
                )?;
                cache.render(&self.inner, D3DRS_FOGSTART, start.to_bits())?;
                cache.render(&self.inner, D3DRS_FOGEND, end.max(start + 0.01).to_bits())?;
            }
            for sampler in 0..2 {
                let filter = if state.linear || sampler == 1 {
                    D3DTEXF_LINEAR
                } else {
                    D3DTEXF_POINT
                };
                let anisotropic = sampler == 0 && state.linear && self.anisotropy > 1;
                cache.sampler(
                    &self.inner,
                    sampler,
                    D3DSAMP_MINFILTER,
                    if anisotropic {
                        D3DTEXF_ANISOTROPIC.0 as u32
                    } else {
                        filter.0 as u32
                    },
                )?;
                cache.sampler(
                    &self.inner,
                    sampler,
                    D3DSAMP_MAXANISOTROPY,
                    if anisotropic { self.anisotropy } else { 1 },
                )?;
                cache.sampler(&self.inner, sampler, D3DSAMP_MAGFILTER, filter.0 as u32)?;
                cache.sampler(
                    &self.inner,
                    sampler,
                    D3DSAMP_MIPFILTER,
                    if state.mipmaps && sampler == 0 {
                        if state.linear
                            && self.caps.raw.TextureFilterCaps & D3DPTFILTERCAPS_MIPFLINEAR as u32
                                != 0
                        {
                            D3DTEXF_LINEAR.0
                        } else {
                            D3DTEXF_POINT.0
                        }
                    } else {
                        D3DTEXF_NONE.0
                    } as u32,
                )?;
                let address = if sampler == 0 && state.wrap || sampler == 1 && state.glint != 0 {
                    D3DTADDRESS_WRAP
                } else {
                    D3DTADDRESS_CLAMP
                };
                cache.sampler(&self.inner, sampler, D3DSAMP_ADDRESSU, address.0 as u32)?;
                cache.sampler(&self.inner, sampler, D3DSAMP_ADDRESSV, address.0 as u32)?;
            }
        }
        self.cached_state = Some(key);
        Ok(())
    }
    pub fn draw(&mut self, mesh: &Mesh, first: u32, count: u32) -> Result<()> {
        if !Rc::ptr_eq(&mesh.owner, &self.inner)
            || first % 4 != 0
            || count % 4 != 0
            || first.checked_add(count).is_none_or(|n| n > mesh.filled)
        {
            return Err("D3D9 mesh draw bounds/device mismatch".into());
        }
        if count == 0 {
            return Ok(());
        }
        self.vertex_pipeline(mesh.encoding)?;
        self.shared_indices()?;
        let raw = mesh.raw.borrow();
        let raw = raw
            .as_ref()
            .ok_or("D3D9 terrain buffer needs re-upload after Reset")?;
        // SAFETY: range checked above; shared indices start at zero relative to the base vertex.
        unsafe {
            self.state_cache.bind_stream(
                &self.inner,
                raw,
                mesh.stride() as u32,
                crate::development_build() && self.caps.software_vertices,
            )?;
            profile_call!(
                self.inner,
                DrawIndexedPrimitive,
                self.inner.device.DrawIndexedPrimitive(
                    D3DPT_TRIANGLELIST,
                    first as i32,
                    0,
                    count,
                    0,
                    count / 2,
                )
            )?;
        }
        profile_count!(self.inner, |c| c.draw(true, count, count / 2, 0, false));
        self.record_draw();
        Ok(())
    }
    pub fn draw_transient(&mut self, vertices: &[Vertex]) -> Result<()> {
        self.transient(vertices, 0)
    }
    pub fn draw_triangles(&mut self, vertices: &[Vertex]) -> Result<()> {
        self.transient(vertices, 1)
    }
    pub fn draw_lines(&mut self, vertices: &[Vertex]) -> Result<()> {
        self.transient(vertices, 2)
    }
    fn transient(&mut self, vertices: &[Vertex], kind: u8) -> Result<()> {
        let primitive = match kind {
            1 => 3,
            2 => 2,
            _ => 4,
        };
        if !vertices.len().is_multiple_of(primitive) {
            return Err("D3D9 incomplete primitive".into());
        }
        self.vertex_pipeline(VertexEncoding::Full)?;
        let limit = self.max_vertices() / primitive * primitive;
        for chunk in vertices.chunks(limit) {
            if chunk.is_empty() {
                continue;
            }
            if kind == 0 {
                self.shared_indices()?;
            }
            let base = self.stage_vertices(chunk)?;
            let buffer = self
                .transient
                .as_ref()
                .ok_or("D3D9 dynamic vertex buffer missing")?;
            // SAFETY: the staged range fits the live buffer and is a complete primitive.
            unsafe {
                self.state_cache.bind_stream(
                    &self.inner,
                    buffer,
                    size_of::<Vertex>() as u32,
                    crate::development_build() && self.caps.software_vertices,
                )?;
                if kind == 2 {
                    profile_call!(
                        self.inner,
                        DrawPrimitive,
                        self.inner.device.DrawPrimitive(
                            D3DPT_LINELIST,
                            base,
                            chunk.len() as u32 / 2,
                        )
                    )?;
                } else if kind == 1 {
                    profile_call!(
                        self.inner,
                        DrawPrimitive,
                        self.inner.device.DrawPrimitive(
                            D3DPT_TRIANGLELIST,
                            base,
                            chunk.len() as u32 / 3,
                        )
                    )?;
                } else {
                    profile_call!(
                        self.inner,
                        DrawIndexedPrimitive,
                        self.inner.device.DrawIndexedPrimitive(
                            D3DPT_TRIANGLELIST,
                            base as i32,
                            0,
                            chunk.len() as u32,
                            0,
                            chunk.len() as u32 / 2,
                        )
                    )?;
                }
            }
            profile_count!(self.inner, |c| c.draw(
                kind == 0,
                chunk.len() as u32,
                if kind == 2 {
                    0
                } else if kind == 1 {
                    chunk.len() as u32 / 3
                } else {
                    chunk.len() as u32 / 2
                },
                if kind == 2 { chunk.len() as u32 / 2 } else { 0 },
                false
            ));
            self.record_draw();
        }
        Ok(())
    }
    /// Upload ordered entity quad batches once without expanding them to triangles.
    pub fn stage_quads(&mut self, vertices: &[Vertex]) -> Result<StagedQuads> {
        self.stage_quad_slices(&[vertices])
    }
    /// Write the input slices in order into one ring reservation and one lock.
    pub fn stage_quad_slices(&mut self, slices: &[&[Vertex]]) -> Result<StagedQuads> {
        let count = crate::geometry::staged_quad_count(slices, self.max_vertices())
            .ok_or("D3D9 staged quad slices outside alignment or buffer capacity")?;
        let first = self.stage_vertex_slices(slices, count)?;
        Ok(StagedQuads {
            owner: self.inner.clone(),
            serial: self.transient_serial,
            first,
            count: count as u32,
        })
    }
    pub fn draw_staged_quads(
        &mut self,
        vertices: &StagedQuads,
        range: std::ops::Range<u32>,
    ) -> Result<()> {
        if !Rc::ptr_eq(&vertices.owner, &self.inner) || vertices.serial != self.transient_serial {
            return Err("D3D9 stale staged quad stream".into());
        }
        let (base, count) =
            crate::geometry::staged_quad_range(vertices.first, vertices.count, range)
                .ok_or("D3D9 invalid staged quad range")?;
        if count == 0 {
            return Ok(());
        }
        self.vertex_pipeline(VertexEncoding::Full)?;
        self.shared_indices()?;
        let buffer = self
            .transient
            .as_ref()
            .ok_or("D3D9 missing staged quad buffer")?;
        // SAFETY: checked device/generation/range; stage_vertices bounds the
        // stream to shared INDEX16 capacity. NumVertices stays tight per draw.
        unsafe {
            self.state_cache.bind_stream(
                &self.inner,
                buffer,
                size_of::<Vertex>() as u32,
                crate::development_build() && self.caps.software_vertices,
            )?;
            profile_call!(
                self.inner,
                DrawIndexedPrimitive,
                self.inner.device.DrawIndexedPrimitive(
                    D3DPT_TRIANGLELIST,
                    base,
                    0,
                    count,
                    0,
                    count / 2
                )
            )?;
        }
        profile_count!(self.inner, |c| c.draw(true, count, count / 2, 0, false));
        self.record_draw();
        Ok(())
    }
    /// Upload a complete HUD once, then draw its ordered state/texture ranges.
    /// Larger batches can use draw_triangles, which splits without dropping content.
    pub fn stage_triangles(&mut self, vertices: &[Vertex]) -> Result<StagedTriangles> {
        if vertices.len() % 3 != 0 {
            return Err("D3D9 incomplete triangle".into());
        }
        let first = self.stage_vertices(vertices)?;
        Ok(StagedTriangles {
            owner: self.inner.clone(),
            serial: self.transient_serial,
            first,
            count: vertices.len() as u32,
        })
    }
    pub fn draw_staged_triangles(
        &mut self,
        vertices: &StagedTriangles,
        range: std::ops::Range<u32>,
    ) -> Result<()> {
        if !Rc::ptr_eq(&vertices.owner, &self.inner)
            || vertices.serial != self.transient_serial
            || range.start > range.end
            || range.end > vertices.count
            || range.start % 3 != 0
            || range.end % 3 != 0
        {
            return Err("D3D9 stale or invalid staged triangle range".into());
        }
        if range.is_empty() {
            return Ok(());
        }
        self.vertex_pipeline(VertexEncoding::Full)?;
        let buffer = self
            .transient
            .as_ref()
            .ok_or("D3D9 missing staged buffer")?;
        // SAFETY: owner, upload generation and bounds were checked; no raw resources escape.
        unsafe {
            self.state_cache.bind_stream(
                &self.inner,
                buffer,
                size_of::<Vertex>() as u32,
                crate::development_build() && self.caps.software_vertices,
            )?;
            profile_call!(
                self.inner,
                DrawPrimitive,
                self.inner.device.DrawPrimitive(
                    D3DPT_TRIANGLELIST,
                    vertices.first + range.start,
                    range.len() as u32 / 3,
                )
            )?;
        }
        profile_count!(self.inner, |c| c.draw(
            false,
            range.len() as u32,
            range.len() as u32 / 3,
            0,
            false
        ));
        self.record_draw();
        Ok(())
    }
    fn stage_vertices(&mut self, vertices: &[Vertex]) -> Result<u32> {
        self.stage_vertex_slices(&[vertices], vertices.len())
    }
    // Count is the sum of the lengths of these immutable slices, either from
    // the single-slice caller or the checked quad count.
    fn stage_vertex_slices(&mut self, slices: &[&[Vertex]], count: usize) -> Result<u32> {
        if count == 0 || count > self.max_vertices() {
            return Err("D3D9 transient upload outside buffer capacity".into());
        }
        debug_assert_eq!(count, slices.iter().map(|s| s.len()).sum::<usize>());
        if self.transient.is_none() {
            let mut buffer = None;
            self.transient_capacity = self.max_vertices() * size_of::<Vertex>();
            self.transient_ring.reset();
            // SAFETY: bounded capacity, creator thread; released before Reset.
            unsafe {
                profile_call!(
                    self.inner,
                    CreateVertexBuffer,
                    self.inner.device.CreateVertexBuffer(
                        self.transient_capacity as u32,
                        (D3DUSAGE_DYNAMIC | D3DUSAGE_WRITEONLY) as u32
                            | if self.caps.software_vertices {
                                D3DUSAGE_SOFTWAREPROCESSING as u32
                            } else {
                                0
                            },
                        FVF,
                        D3DPOOL_DEFAULT,
                        &mut buffer,
                        ptr::null_mut(),
                    )
                )?;
            }
            self.transient = buffer;
        }
        let serial = self
            .inner
            .stream_sequence
            .get()
            .checked_add(1)
            .ok_or("D3D9 upload sequence exhausted")?;
        self.inner.stream_sequence.set(serial);
        self.transient_serial = serial;
        let bytes = count * size_of::<Vertex>();
        let (offset, discard) = self
            .transient_ring
            .reserve(bytes, self.transient_capacity)
            .ok_or("D3D9 transient ring overflow")?;
        if discard {
            self.state_cache.stream.invalidate();
        }
        let buffer = self
            .transient
            .as_ref()
            .ok_or("D3D9 dynamic vertex buffer missing")?;
        // SAFETY: the immutable slices' total size matches the checked lock.
        // NOOVERWRITE appends to untouched storage. DISCARD permits renaming.
        unsafe {
            let mut memory = ptr::null_mut();
            profile_call!(
                self.inner,
                LockVertex,
                buffer.Lock(
                    offset as u32,
                    bytes as u32,
                    &mut memory,
                    if discard {
                        D3DLOCK_DISCARD
                    } else {
                        D3DLOCK_NOOVERWRITE
                    } as u32,
                )
            )?;
            if memory.is_null() {
                profile_call!(self.inner, UnlockVertex, buffer.Unlock())?;
                return Err("D3D9 empty transient lock".into());
            }
            let mut dest = memory.cast::<u8>();
            for vertices in slices {
                let source = bytemuck::cast_slice::<_, u8>(vertices);
                ptr::copy_nonoverlapping(source.as_ptr(), dest, source.len());
                dest = dest.add(source.len());
            }
            profile_call!(self.inner, UnlockVertex, buffer.Unlock())?;
        }
        self.uploads += bytes as u64;
        self.stream_uploads += bytes as u64;
        self.stream_locks += 1;
        profile_count!(self.inner, |c| c.upload_bytes[1] += bytes as u64);
        Ok((offset / size_of::<Vertex>()) as u32)
    }
    /// Capture only on request. Readback is deliberately absent from the normal frame path.
    pub fn screenshot(&self) -> Result<(u32, u32, Vec<u8>)> {
        // SAFETY: temporary backbuffer and SYSTEMMEM surfaces are released before returning/Reset.
        unsafe {
            let source = self.inner.device.GetRenderTarget(0)?;
            let mut desc = D3DSURFACE_DESC::default();
            source.GetDesc(&mut desc)?;
            let rgb565 = desc.Format == D3DFMT_R5G6B5;
            if !rgb565 && desc.Format != D3DFMT_X8R8G8B8 && desc.Format != D3DFMT_A8R8G8B8 {
                return Err("D3D9 screenshot requires an RGB565 or 32-bit backbuffer".into());
            }
            let stride = if rgb565 { 2 } else { 4 };
            let mut dest = None;
            self.inner.device.CreateOffscreenPlainSurface(
                desc.Width,
                desc.Height,
                desc.Format,
                D3DPOOL_SYSTEMMEM,
                &mut dest,
                ptr::null_mut(),
            )?;
            let dest = dest.ok_or("D3D9 readback surface missing")?;
            self.inner.device.GetRenderTargetData(&source, &dest)?;
            let mut locked = D3DLOCKED_RECT::default();
            dest.LockRect(&mut locked, ptr::null(), D3DLOCK_READONLY as u32)?;
            if locked.pBits.is_null() || locked.Pitch < (desc.Width * stride) as i32 {
                dest.UnlockRect()?;
                return Err("D3D9 readback returned an invalid pointer/pitch".into());
            }
            let mut rgba = vec![0u8; desc.Width as usize * desc.Height as usize * 4];
            for y in 0..desc.Height as usize {
                let row = std::slice::from_raw_parts(
                    locked.pBits.cast::<u8>().add(y * locked.Pitch as usize),
                    desc.Width as usize * stride as usize,
                );
                crate::readback::rgba_row(
                    row,
                    &mut rgba[y * desc.Width as usize * 4..(y + 1) * desc.Width as usize * 4],
                    rgb565,
                );
            }
            dest.UnlockRect()?;
            Ok((desc.Width, desc.Height, rgba))
        }
    }
}
impl Vertex {
    const fn zero() -> Self {
        Self {
            pos: [0.0; 3],
            color: 0,
            uv: [0.0; 2],
            light: [0.0; 2],
        }
    }
}

/// Clear texels without reading or writing row padding.
///
/// # Safety
/// `dest` must name writable locked texture rows. `pitch` must be at least
/// `row_bytes`, and each row's texel bytes must be within the same allocation.
unsafe fn zero_texture_rows(dest: *mut u8, row_bytes: usize, pitch: usize, height: usize) {
    // SAFETY: the caller guarantees every row is writable. Merge contiguous
    // rows into one clear when the driver has not added row padding.
    unsafe {
        if pitch == row_bytes {
            ptr::write_bytes(dest, 0, row_bytes * height);
        } else {
            for row in 0..height {
                ptr::write_bytes(dest.add(row * pitch), 0, row_bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::zero_texture_rows;

    #[test]
    fn transparent_clear_keeps_pitch_padding_and_surrounding_bytes() {
        for pitch in [8, 12] {
            let mut bytes = [123u8; 40];
            let mut expected = bytes;
            for row in 0..3 {
                let start = 4 + row * pitch;
                expected[start..start + 8].fill(0);
            }
            // SAFETY: all three rows fit inside `bytes` and are disjoint.
            unsafe { zero_texture_rows(bytes.as_mut_ptr().add(4), 8, pitch, 3) };
            assert_eq!(bytes, expected);
        }
    }
}

pub fn rgba_to_argb(rgba: u32) -> u32 {
    (rgba & 0xff00ff00) | (rgba & 0xff) << 16 | (rgba >> 16 & 0xff)
}
pub fn argb(rgba: [f32; 4]) -> u32 {
    let [r, g, b, a] = rgba.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u32);
    a << 24 | r << 16 | g << 8 | b
}

/// A loss may occur after the initial cooperative-level check, during upload/draw.
pub fn is_device_lost(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<windows::core::Error>()
        .is_some_and(|e| e.code() == D3DERR_DEVICELOST || e.code() == D3DERR_DEVICENOTRESET)
}
