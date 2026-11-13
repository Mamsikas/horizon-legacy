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
#[path = "terrain_device.rs"]
mod terrain_device;
use terrain_device::TerrainPipeline;
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
    pub fog: Option<Fog>,
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
            fog: None,
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
    pub raw: D3DCAPS9,
}
type MeshHandle = RefCell<Option<IDirect3DVertexBuffer9>>;
struct Inner {
    stream_sequence: Cell<u64>,
    default_meshes: RefCell<Vec<Weak<MeshHandle>>>,
    presentation: RefCell<Option<Presentation>>,
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
}
pub struct Mesh {
    raw: Rc<MeshHandle>,
    owner: Rc<Inner>,
    pub vertices: u32,
    filled: u32,
    encoding: VertexEncoding,
}
/// Immutable visibility indices. Managed storage survives a real device Reset;
/// the runtime owns the small CPU copy instead of a second application Vec.
pub struct Indices {
    raw: IDirect3DIndexBuffer9,
    owner: Rc<Inner>,
    start: u32,
    end: u32,
    count: u32,
}
impl Indices {
    pub fn bytes(&self) -> u64 {
        u64::from(self.count) * 2
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
    sampler: [[Option<u32>; 14]; 2],
    textures: [usize; 2],
}
impl Default for StateCache {
    fn default() -> Self {
        Self {
            render: [None; 256],
            stage: [[None; 33]; 3],
            sampler: [[None; 14]; 2],
            textures: [usize::MAX; 2],
        }
    }
}
impl StateCache {
    fn render(
        &mut self,
        device: &IDirect3DDevice9,
        state: D3DRENDERSTATETYPE,
        value: u32,
    ) -> windows::core::Result<()> {
        let slot = &mut self.render[state.0 as usize];
        if *slot != Some(value) {
            // SAFETY: a live owned device, with a documented render-state value.
            unsafe {
                device.SetRenderState(state, value)?;
            }
            *slot = Some(value);
        }
        Ok(())
    }
    fn stage(
        &mut self,
        device: &IDirect3DDevice9,
        index: u32,
        state: D3DTEXTURESTAGESTATETYPE,
        value: u32,
    ) -> windows::core::Result<()> {
        let slot = &mut self.stage[index as usize][state.0 as usize];
        if *slot != Some(value) {
            // SAFETY: stages 0/1 were checked in caps; device and constants are live.
            unsafe {
                device.SetTextureStageState(index, state, value)?;
            }
            *slot = Some(value);
        }
        Ok(())
    }
    fn sampler(
        &mut self,
        device: &IDirect3DDevice9,
        index: u32,
        state: D3DSAMPLERSTATETYPE,
        value: u32,
    ) -> windows::core::Result<()> {
        let slot = &mut self.sampler[index as usize][state.0 as usize];
        if *slot != Some(value) {
            // SAFETY: samplers 0/1 and their D3D9 values were checked in caps.
            unsafe {
                device.SetSamplerState(index, state, value)?;
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
pub struct Device {
    inner: Rc<Inner>,
    pub caps: Caps,
    present: D3DPRESENT_PARAMETERS,
    indices: Option<IDirect3DIndexBuffer9>,
    transient: Option<IDirect3DVertexBuffer9>,
    transient_capacity: usize,
    transient_ring: Ring,
    transient_serial: u64,
    dynamic_indices: Option<IDirect3DIndexBuffer9>,
    index_ring: Ring,
    using_dynamic_indices: bool,
    reset_pending: bool,
    surface_pending: bool,
    lost: bool,
    scene: bool,
    cached_state: Option<(DrawState, usize, usize)>,
    state_cache: StateCache,
    cached_transform: Option<[[f32; 16]; 3]>,
    fog_shader: Option<IDirect3DPixelShader9>,
    shader_active: bool,
    fog_constants: Option<Fog>,
    terrain: Option<TerrainPipeline>,
    terrain_encoding: Option<VertexEncoding>,
    vertex_constants: [[f32; 4]; 8],
    vertex_constants_dirty: bool,
    pub draws: u64,
    pub binds: u64,
    pub uploads: u64,
    pub resets: u64,
    pub mesh_uploads: u64,
    pub stream_uploads: u64,
    pub stream_locks: u64,
    pub index_uploads: u64,
}
impl Device {
    pub fn new(window: Arc<Window>, vsync: bool) -> Result<Self> {
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
        let backbuffer = [D3DFMT_X8R8G8B8, D3DFMT_A8R8G8B8]
            .into_iter()
            .find(|&format| {
                // SAFETY: capability query with no user pointers. Keep readback at 32 bits,
                // including when the desktop itself uses a 16-bit display format.
                unsafe {
                    d3d.CheckDeviceType(0, D3DDEVTYPE_HAL, mode.Format, format, BOOL(1))
                        .is_ok()
                }
            })
            .ok_or("D3D9 HAL: no windowed 32-bit backbuffer compatible with the desktop format")?;
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
        let depth = [D3DFMT_D24X8, D3DFMT_D16]
            .into_iter()
            .find(|&depth| {
                // SAFETY: capability probes only; no resources or pointers escape.
                unsafe {
                    d3d.CheckDeviceFormat(
                        0,
                        D3DDEVTYPE_HAL,
                        mode.Format,
                        D3DUSAGE_DEPTHSTENCIL as u32,
                        D3DRTYPE_SURFACE,
                        depth,
                    )
                    .is_ok()
                        && d3d
                            .CheckDepthStencilMatch(
                                0,
                                D3DDEVTYPE_HAL,
                                mode.Format,
                                backbuffer,
                                depth,
                            )
                            .is_ok()
                }
            })
            .ok_or("D3D9 HAL: no compatible D24X8 or D16 depth buffer")?;
        let software_vertices = raw.DevCaps & D3DDEVCAPS_HWTRANSFORMANDLIGHT as u32 == 0;
        let requested = std::env::var("HORIZON_D3D9_VERTEX_PROCESSING").unwrap_or_default();
        let software_vertices = match requested.as_str() {
            "software" => true,
            "hardware" if software_vertices => {
                return Err(
                    "D3D9: hardware vertex processing requested but HWTRANSFORMANDLIGHT is absent"
                        .into(),
                );
            }
            "" | "hardware" => software_vertices,
            _ => return Err("HORIZON_D3D9_VERTEX_PROCESSING must be hardware or software".into()),
        };
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
            mode.Format,
            depth,
            available_texture_bytes
        );
        let mut result = Self {
            inner: Rc::new(Inner {
                stream_sequence: Cell::new(0),
                default_meshes: RefCell::new(Vec::new()),
                presentation: RefCell::new(None),
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
            dynamic_indices: None,
            index_ring: Ring::default(),
            using_dynamic_indices: false,
            reset_pending: false,
            surface_pending: true,
            lost: false,
            scene: false,
            cached_state: None,
            state_cache: StateCache::default(),
            cached_transform: None,
            fog_shader: None,
            shader_active: false,
            fog_constants: None,
            terrain: None,
            terrain_encoding: None,
            vertex_constants: [[0.0; 4]; 8],
            vertex_constants_dirty: true,
            draws: 0,
            binds: 0,
            uploads: 0,
            resets: 0,
            mesh_uploads: 0,
            stream_uploads: 0,
            stream_locks: 0,
            index_uploads: 0,
        };
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
                    .CreatePixelShader(crate::fog::bytecode().as_ptr())
            } {
                Ok(shader) => {
                    result.fog_shader = Some(shader);
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
            dynamic_indices: None,
            index_ring: Ring::default(),
            using_dynamic_indices: false,
            reset_pending: false,
            surface_pending: true,
            lost: self.lost,
            scene: false,
            cached_state: None,
            state_cache: StateCache::default(),
            cached_transform: None,
            fog_shader: self.fog_shader.clone(),
            shader_active: false,
            fog_constants: None,
            terrain: self.terrain.clone(),
            terrain_encoding: None,
            vertex_constants: [[0.0; 4]; 8],
            vertex_constants_dirty: true,
            draws: 0,
            binds: 0,
            uploads: 0,
            resets: self.resets,
            mesh_uploads: 0,
            stream_uploads: 0,
            stream_locks: 0,
            index_uploads: 0,
        }
    }
    pub fn compact_vertices(&self) -> bool {
        self.terrain.is_some()
    }
    fn vertex_pipeline(&mut self, encoding: VertexEncoding) -> Result<()> {
        if encoding == VertexEncoding::Full {
            if self.terrain_encoding.take().is_some() {
                // SAFETY: the canonical FVF matches every transient/full vertex.
                unsafe {
                    self.inner.device.SetVertexShader(None)?;
                    self.inner.device.SetFVF(FVF)?;
                }
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
                self.inner.device.SetVertexShader(&pipeline.shader)?;
                self.inner.device.SetVertexDeclaration(match encoding {
                    VertexEncoding::Compact => &pipeline.compact,
                    _ => &pipeline.wide,
                })?;
            }
            self.terrain_encoding = Some(encoding);
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
            self.inner.device.CreateIndexBuffer(
                (indices.len() * 2) as u32,
                D3DUSAGE_WRITEONLY as u32,
                D3DFMT_INDEX16,
                D3DPOOL_MANAGED,
                &mut buffer,
                ptr::null_mut(),
            )?;
            let buffer = buffer.ok_or("D3D9 index buffer missing")?;
            let mut memory = ptr::null_mut();
            buffer.Lock(0, (indices.len() * 2) as u32, &mut memory, 0)?;
            ptr::copy_nonoverlapping(
                indices.as_ptr().cast::<u8>(),
                memory.cast::<u8>(),
                indices.len() * 2,
            );
            buffer.Unlock()?;
            self.indices = Some(buffer);
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
        if self.reset_pending {
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
            self.dynamic_indices = None;
            self.index_ring.reset();
            // SAFETY: no outstanding lock, scene, stateblock or backbuffer reference exists.
            unsafe {
                self.inner.device.SetStreamSource(0, None, 0, 0)?;
                self.inner.device.SetIndices(None)?;
                self.inner.device.SetTexture(0, None)?;
                self.inner.device.SetTexture(1, None)?;
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
        if self.surface_pending || !matches {
            if let Err(error) = self.create_presentation() {
                if is_device_lost(error.as_ref()) {
                    self.lost = true;
                    return Ok(false);
                }
                return Err(error);
            }
        }
        self.using_dynamic_indices = false;
        self.mesh_uploads = 0;
        self.stream_uploads = 0;
        self.stream_locks = 0;
        self.transient_serial = 0;
        self.index_uploads = 0;
        self.draws = 0;
        self.binds = 0;
        self.terrain_encoding = None;
        self.vertex_constants_dirty = true;
        self.cached_state = None;
        self.state_cache = StateCache::default();
        self.cached_transform = None;
        self.shader_active = false;
        self.fog_constants = None;
        // SAFETY: state values are D3D9 constants; begin/finish pair on this thread.
        unsafe {
            self.inner.device.SetVertexShader(None)?;
            self.inner.device.SetPixelShader(None)?;
            self.inner.device.SetFVF(FVF)?;
            self.inner.device.SetIndices(self.indices.as_ref())?;
            self.inner.device.SetRenderState(D3DRS_LIGHTING, 0)?;
            self.inner
                .device
                .SetRenderState(D3DRS_ZFUNC, D3DCMP_GREATEREQUAL.0 as u32)?;
            self.inner
                .device
                .SetRenderState(D3DRS_ALPHAFUNC, D3DCMP_GREATER.0 as u32)?;
            self.inner.device.SetRenderState(D3DRS_ALPHAREF, 25)?;
            self.inner.device.SetRenderState(D3DRS_DITHERENABLE, 1)?;
            // Clear obeys the previous frame's scissor too, including GUI item clips.
            self.inner
                .device
                .SetRenderState(D3DRS_SCISSORTESTENABLE, 0)?;
            self.inner.device.Clear(
                0,
                ptr::null(),
                D3DCLEAR_TARGET as u32 | D3DCLEAR_ZBUFFER as u32,
                argb([color[0], color[1], color[2], 1.0]),
                0.0,
                0,
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
            .render(&self.inner.device, D3DRS_SCISSORTESTENABLE, 0)?;
        self.cached_state = None;
        // SAFETY: live depth surface; clear to the caller's depth convention's far value.
        unsafe {
            self.inner
                .device
                .Clear(0, ptr::null(), D3DCLEAR_ZBUFFER as u32, 0, far, 0)?;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<bool> {
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
            Ok(()) => Ok(true),
        }
    }
    pub fn cancel_frame(&mut self) {
        if self.scene {
            // SAFETY: best-effort matching EndScene on an error path; never called twice.
            let _ = unsafe { self.inner.device.EndScene() };
            self.scene = false;
        }
    }
    pub fn mark_lost(&mut self) {
        self.lost = true;
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
                self.inner.device.SetTransform(
                    D3DTS_WORLD,
                    &std::mem::transmute::<[f32; 16], Matrix4x4>(world),
                )?;
            }
            if previous.is_none_or(|p| p[1] != view) {
                self.inner.device.SetTransform(
                    D3DTS_VIEW,
                    &std::mem::transmute::<[f32; 16], Matrix4x4>(view),
                )?;
                if self.fog_shader.is_some() {
                    let inverse = glam::Mat4::from_cols_array(&view).inverse().to_cols_array();
                    self.inner.device.SetTransform(
                        D3DTS_TEXTURE2,
                        &std::mem::transmute::<[f32; 16], Matrix4x4>(inverse),
                    )?;
                }
            }
            if previous.is_none_or(|p| p[2] != projection) {
                self.inner.device.SetTransform(
                    D3DTS_PROJECTION,
                    &std::mem::transmute::<[f32; 16], Matrix4x4>(projection),
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
        if width == 0
            || height == 0
            || width > self.caps.max_texture
            || height > self.caps.max_texture
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err(format!(
                "D3D9 texture: invalid {}x{}, {} bytes (caps {})",
                width,
                height,
                rgba.len(),
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
        self.patch(&texture, 0, 0, 0, width, height, rgba)?;
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
        if !Rc::ptr_eq(&texture.owner, &self.inner)
            || level >= texture.levels
            || width == 0
            || height == 0
            || x.checked_add(width)
                .is_none_or(|v| v > (texture.width >> level).max(1))
            || y.checked_add(height)
                .is_none_or(|v| v > (texture.height >> level).max(1))
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err("D3D9 texture patch bounds/device mismatch".into());
        }
        let rect = RECT {
            left: x as i32,
            top: y as i32,
            right: (x + width) as i32,
            bottom: (y + height) as i32,
        };
        let mut locked = D3DLOCKED_RECT::default();
        // SAFETY: validated input length and rect. Copy rows using the runtime's pitch, converting RGBA to BGRA.
        unsafe {
            texture.raw.LockRect(level, &mut locked, &rect, 0)?;
            if locked.pBits.is_null() || locked.Pitch < (width * 4) as i32 {
                texture.raw.UnlockRect(level)?;
                return Err("D3D9 invalid texture lock pitch".into());
            }
            for row in 0..height as usize {
                let dest = std::slice::from_raw_parts_mut(
                    locked.pBits.cast::<u8>().add(row * locked.Pitch as usize),
                    width as usize * 4,
                );
                let source = &rgba[row * width as usize * 4..(row + 1) * width as usize * 4];
                for (d, s) in dest.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
                }
            }
            texture.raw.UnlockRect(level)?;
        }
        self.uploads += rgba.len() as u64;
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
                D3DPOOL_DEFAULT,
                &mut raw,
                ptr::null_mut(),
            )?;
            let raw = raw.ok_or("D3D9 vertex buffer missing")?;
            let mut memory = ptr::null_mut();
            raw.Lock(0, bytes.len() as u32, &mut memory, 0)?;
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            raw.Unlock()?;
            self.uploads += bytes.len() as u64;
            self.mesh_uploads += bytes.len() as u64;
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
                self.inner.device.SetIndices(self.indices.as_ref())?;
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
            indices.Lock(
                offset as u32,
                bytes.len() as u32,
                &mut memory,
                if discard {
                    D3DLOCK_DISCARD
                } else {
                    D3DLOCK_NOOVERWRITE
                } as u32,
            )?;
            if memory.is_null() {
                indices.Unlock()?;
                return Err("D3D9 empty index lock".into());
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            indices.Unlock()?;
            self.inner.device.SetIndices(indices)?;
            self.using_dynamic_indices = true;
            self.inner
                .device
                .SetStreamSource(0, raw, 0, mesh.stride() as u32)?;
            self.inner.device.DrawIndexedPrimitive(
                D3DPT_TRIANGLELIST,
                0,
                batch.start_vertex(),
                batch.end_vertex() - batch.start_vertex(),
                offset as u32 / 2,
                batch.indices().len() as u32 / 3,
            )?;
        }
        self.uploads += bytes.len() as u64;
        self.index_uploads += bytes.len() as u64;
        self.draws += 1;
        Ok(())
    }
    pub fn cache_indices(&mut self, batch: &IndexBatch) -> Result<Indices> {
        if batch.indices().is_empty()
            || batch.indices().len() / 3 > self.caps.max_primitive_count as usize
        {
            return Err("D3D9 cached index batch exceeds caps".into());
        }
        let bytes = bytemuck::cast_slice::<_, u8>(batch.indices());
        let mut raw = None;
        // SAFETY: complete validated quad indices and exact lock/copy sizes.
        unsafe {
            self.inner.device.CreateIndexBuffer(
                bytes.len() as u32,
                D3DUSAGE_WRITEONLY as u32,
                D3DFMT_INDEX16,
                D3DPOOL_MANAGED,
                &mut raw,
                ptr::null_mut(),
            )?;
            let raw = raw.ok_or("D3D9 cached index buffer missing")?;
            let mut memory = ptr::null_mut();
            raw.Lock(0, bytes.len() as u32, &mut memory, 0)?;
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            raw.Unlock()?;
            self.uploads += bytes.len() as u64;
            self.index_uploads += bytes.len() as u64;
            Ok(Indices {
                raw,
                owner: self.inner.clone(),
                start: batch.start_vertex(),
                end: batch.end_vertex(),
                count: batch.indices().len() as u32,
            })
        }
    }
    pub fn draw_cached(&mut self, mesh: &Mesh, indices: &Indices) -> Result<()> {
        if !Rc::ptr_eq(&mesh.owner, &self.inner)
            || !Rc::ptr_eq(&indices.owner, &self.inner)
            || indices.end > mesh.filled
        {
            return Err("D3D9 cached draw bounds/device mismatch".into());
        }
        self.vertex_pipeline(mesh.encoding)?;
        let raw = mesh.raw.borrow();
        let raw = raw.as_ref().ok_or("D3D9 arena requires re-upload")?;
        // SAFETY: index/vertex ranges were validated against this device and mesh.
        unsafe {
            self.inner.device.SetIndices(&indices.raw)?;
            self.inner
                .device
                .SetStreamSource(0, raw, 0, mesh.stride() as u32)?;
            self.inner.device.DrawIndexedPrimitive(
                D3DPT_TRIANGLELIST,
                0,
                indices.start,
                indices.end - indices.start,
                0,
                indices.count / 3,
            )?;
        }
        self.using_dynamic_indices = true;
        self.draws += 1;
        Ok(())
    }
    pub fn state(
        &mut self,
        state: DrawState,
        texture: &Texture,
        lightmap: Option<&Texture>,
    ) -> Result<()> {
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
        self.cached_state = Some(key);
        // SAFETY: validated resource ownership and documented D3D9 state constants.
        unsafe {
            let d = &self.inner.device;
            let cache = &mut self.state_cache;
            if cache.textures[0] != texture.raw.as_raw() as usize {
                d.SetTexture(0, &texture.base)?;
                cache.textures[0] = texture.raw.as_raw() as usize;
                self.binds += 1;
            }
            cache.stage(d, 0, D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32)?;
            cache.stage(d, 0, D3DTSS_COLORARG1, D3DTA_TEXTURE)?;
            cache.stage(d, 0, D3DTSS_COLORARG2, D3DTA_DIFFUSE)?;
            cache.stage(d, 0, D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32)?;
            cache.stage(d, 0, D3DTSS_ALPHAARG1, D3DTA_TEXTURE)?;
            cache.stage(d, 0, D3DTSS_ALPHAARG2, D3DTA_DIFFUSE)?;
            cache.stage(d, 0, D3DTSS_TEXCOORDINDEX, 0)?;
            let lm = if state.lightmap { lightmap } else { None };
            let light_texture = lm.map(|t| &t.base);
            let light_id = light_texture.map_or(0, |t| t.as_raw() as usize);
            if cache.textures[1] != light_id {
                d.SetTexture(1, light_texture)?;
                cache.textures[1] = light_id;
                self.binds += 1;
            }
            cache.stage(
                d,
                1,
                D3DTSS_COLOROP,
                if lm.is_some() {
                    D3DTOP_MODULATE.0
                } else {
                    D3DTOP_DISABLE.0
                } as u32,
            )?;
            cache.stage(d, 1, D3DTSS_COLORARG1, D3DTA_TEXTURE)?;
            cache.stage(d, 1, D3DTSS_COLORARG2, D3DTA_CURRENT)?;
            cache.stage(d, 1, D3DTSS_ALPHAOP, D3DTOP_SELECTARG2.0 as u32)?;
            cache.stage(d, 1, D3DTSS_ALPHAARG2, D3DTA_CURRENT)?;
            cache.stage(d, 1, D3DTSS_TEXCOORDINDEX, 1)?;
            let (src, dst) = match state.blend {
                Blend::Opaque | Blend::Alpha => (D3DBLEND_SRCALPHA, D3DBLEND_INVSRCALPHA),
                Blend::Additive => (D3DBLEND_SRCALPHA, D3DBLEND_ONE),
                Blend::Invert => (D3DBLEND_INVDESTCOLOR, D3DBLEND_INVSRCCOLOR),
                Blend::Multiply => (D3DBLEND_ZERO, D3DBLEND_INVSRCCOLOR),
                Blend::Cracks => (D3DBLEND_DESTCOLOR, D3DBLEND_SRCCOLOR),
            };
            cache.render(
                d,
                D3DRS_ALPHABLENDENABLE,
                u32::from(state.blend != Blend::Opaque),
            )?;
            cache.render(d, D3DRS_SRCBLEND, src.0 as u32)?;
            cache.render(d, D3DRS_DESTBLEND, dst.0 as u32)?;
            cache.render(d, D3DRS_ZENABLE, u32::from(state.depth_test))?;
            cache.render(d, D3DRS_ZWRITEENABLE, u32::from(state.depth_write))?;
            cache.render(
                d,
                D3DRS_ZFUNC,
                if state.reverse_z {
                    D3DCMP_GREATEREQUAL
                } else {
                    D3DCMP_LESSEQUAL
                }
                .0 as u32,
            )?;
            cache.render(d, D3DRS_ALPHATESTENABLE, u32::from(state.alpha_test))?;
            cache.render(d, D3DRS_ALPHAREF, u32::from(state.alpha_ref))?;
            cache.render(
                d,
                D3DRS_CULLMODE,
                if state.cull {
                    D3DCULL_CW.0
                } else {
                    D3DCULL_NONE.0
                } as u32,
            )?;
            cache.render(
                d,
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
            let shader_fog = state.fog.is_some() && state.lightmap && self.fog_shader.is_some();
            if shader_fog != self.shader_active {
                d.SetPixelShader(if shader_fog {
                    self.fog_shader.as_ref()
                } else {
                    None
                })?;
                self.shader_active = shader_fog;
            }
            if shader_fog {
                cache.stage(
                    d,
                    2,
                    D3DTSS_TEXCOORDINDEX,
                    D3DTSS_TCI_CAMERASPACEPOSITION | 2,
                )?;
                cache.stage(d, 2, D3DTSS_TEXTURETRANSFORMFLAGS, D3DTTFF_COUNT3.0 as u32)?;
                if self.fog_constants != state.fog {
                    let constants = state.fog.unwrap().constants();
                    d.SetPixelShaderConstantF(0, constants.as_ptr().cast(), 3)?;
                    self.fog_constants = state.fog;
                }
            } else if self.fog_shader.is_some() {
                cache.stage(d, 2, D3DTSS_TEXTURETRANSFORMFLAGS, D3DTTFF_DISABLE.0 as u32)?;
                cache.stage(d, 2, D3DTSS_TEXCOORDINDEX, 2)?;
            }
            cache.render(
                d,
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
                cache.render(d, D3DRS_FOGCOLOR, argb([color[0], color[1], color[2], 1.0]))?;
                cache.render(d, D3DRS_FOGTABLEMODE, D3DFOG_NONE.0 as u32)?;
                cache.render(d, D3DRS_FOGVERTEXMODE, D3DFOG_LINEAR.0 as u32)?;
                cache.render(
                    d,
                    D3DRS_RANGEFOGENABLE,
                    u32::from(self.caps.raw.RasterCaps & D3DPRASTERCAPS_FOGRANGE as u32 != 0),
                )?;
                cache.render(d, D3DRS_FOGSTART, start.to_bits())?;
                cache.render(d, D3DRS_FOGEND, end.max(start + 0.01).to_bits())?;
            }
            for sampler in 0..2 {
                let filter = if state.linear || sampler == 1 {
                    D3DTEXF_LINEAR
                } else {
                    D3DTEXF_POINT
                };
                cache.sampler(d, sampler, D3DSAMP_MINFILTER, filter.0 as u32)?;
                cache.sampler(d, sampler, D3DSAMP_MAGFILTER, filter.0 as u32)?;
                cache.sampler(
                    d,
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
                let address = if sampler == 0 && state.wrap {
                    D3DTADDRESS_WRAP
                } else {
                    D3DTADDRESS_CLAMP
                };
                cache.sampler(d, sampler, D3DSAMP_ADDRESSU, address.0 as u32)?;
                cache.sampler(d, sampler, D3DSAMP_ADDRESSV, address.0 as u32)?;
            }
        }
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
            self.inner
                .device
                .SetStreamSource(0, raw, 0, mesh.stride() as u32)?;
            self.inner.device.DrawIndexedPrimitive(
                D3DPT_TRIANGLELIST,
                first as i32,
                0,
                count,
                0,
                count / 2,
            )?;
        }
        self.draws += 1;
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
        if vertices.len() % primitive != 0 {
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
                self.inner
                    .device
                    .SetStreamSource(0, buffer, 0, size_of::<Vertex>() as u32)?;
                if kind == 2 {
                    self.inner.device.DrawPrimitive(
                        D3DPT_LINELIST,
                        base,
                        chunk.len() as u32 / 2,
                    )?;
                } else if kind == 1 {
                    self.inner.device.DrawPrimitive(
                        D3DPT_TRIANGLELIST,
                        base,
                        chunk.len() as u32 / 3,
                    )?;
                } else {
                    self.inner.device.DrawIndexedPrimitive(
                        D3DPT_TRIANGLELIST,
                        base as i32,
                        0,
                        chunk.len() as u32,
                        0,
                        chunk.len() as u32 / 2,
                    )?;
                }
            }
            self.draws += 1;
        }
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
            self.inner
                .device
                .SetStreamSource(0, buffer, 0, size_of::<Vertex>() as u32)?;
            self.inner.device.DrawPrimitive(
                D3DPT_TRIANGLELIST,
                vertices.first + range.start,
                range.len() as u32 / 3,
            )?;
        }
        self.draws += 1;
        Ok(())
    }
    fn stage_vertices(&mut self, vertices: &[Vertex]) -> Result<u32> {
        if vertices.is_empty() || vertices.len() > self.max_vertices() {
            return Err("D3D9 transient upload outside buffer capacity".into());
        }
        if self.transient.is_none() {
            let mut buffer = None;
            self.transient_capacity = self.max_vertices() * size_of::<Vertex>();
            self.transient_ring.reset();
            // SAFETY: bounded capacity, creator thread; released before Reset.
            unsafe {
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
        let bytes = bytemuck::cast_slice::<_, u8>(vertices);
        let (offset, discard) = self
            .transient_ring
            .reserve(bytes.len(), self.transient_capacity)
            .ok_or("D3D9 transient ring overflow")?;
        let buffer = self
            .transient
            .as_ref()
            .ok_or("D3D9 dynamic vertex buffer missing")?;
        // SAFETY: lock size matches the copy. NOOVERWRITE appends to untouched storage;
        // DISCARD lets the driver rename it when the bounded ring wraps.
        unsafe {
            let mut memory = ptr::null_mut();
            buffer.Lock(
                offset as u32,
                bytes.len() as u32,
                &mut memory,
                if discard {
                    D3DLOCK_DISCARD
                } else {
                    D3DLOCK_NOOVERWRITE
                } as u32,
            )?;
            if memory.is_null() {
                buffer.Unlock()?;
                return Err("D3D9 empty transient lock".into());
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            buffer.Unlock()?;
        }
        self.uploads += bytes.len() as u64;
        self.stream_uploads += bytes.len() as u64;
        self.stream_locks += 1;
        Ok((offset / size_of::<Vertex>()) as u32)
    }
    /// Capture only on request. Readback is deliberately absent from the normal frame path.
    pub fn screenshot(&self) -> Result<(u32, u32, Vec<u8>)> {
        // SAFETY: temporary backbuffer and SYSTEMMEM surfaces are released before returning/Reset.
        unsafe {
            let source = self.inner.device.GetRenderTarget(0)?;
            let mut desc = D3DSURFACE_DESC::default();
            source.GetDesc(&mut desc)?;
            if desc.Format != D3DFMT_X8R8G8B8 && desc.Format != D3DFMT_A8R8G8B8 {
                return Err("D3D9 screenshot requires a 32-bit backbuffer".into());
            }
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
            if locked.pBits.is_null() || locked.Pitch < (desc.Width * 4) as i32 {
                dest.UnlockRect()?;
                return Err("D3D9 readback returned an invalid pointer/pitch".into());
            }
            let mut rgba = vec![0u8; desc.Width as usize * desc.Height as usize * 4];
            for y in 0..desc.Height as usize {
                let row = std::slice::from_raw_parts(
                    locked.pBits.cast::<u8>().add(y * locked.Pitch as usize),
                    desc.Width as usize * 4,
                );
                for (out, bgra) in rgba
                    [y * desc.Width as usize * 4..(y + 1) * desc.Width as usize * 4]
                    .chunks_exact_mut(4)
                    .zip(row.chunks_exact(4))
                {
                    out.copy_from_slice(&[bgra[2], bgra[1], bgra[0], 255]);
                }
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
