//! All COM and locked-memory operations stay here. Resources keep their device and
//! window alive and are deliberately !Send/!Sync: D3D9 Reset belongs to its creator.
use std::{
    cell::RefCell,
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
use crate::{IndexBatch, Ring, Vertex};
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
    pub alpha_test: bool,
    pub alpha_ref: u8,
    pub cull: bool,
    pub lightmap: bool,
    pub fog: Option<([f32; 3], f32, f32)>,
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
    default_meshes: RefCell<Vec<Weak<MeshHandle>>>,
    device: IDirect3DDevice9,
    _d3d: IDirect3D9,
    _window: Arc<Window>,
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
}
impl Mesh {
    /// Reset invalidates DEFAULT buffers; their owner can lazily upload its CPU data again.
    pub fn resident(&self) -> bool {
        self.raw.borrow().is_some()
    }
    pub fn bytes(&self) -> u64 {
        u64::from(self.vertices) * size_of::<Vertex>() as u64
    }
}
struct StateCache {
    render: [Option<u32>; 256],
    stage: [[Option<u32>; 33]; 2],
    sampler: [[Option<u32>; 14]; 2],
    textures: [usize; 2],
}
impl Default for StateCache {
    fn default() -> Self {
        Self {
            render: [None; 256],
            stage: [[None; 33]; 2],
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
pub struct Device {
    inner: Rc<Inner>,
    pub caps: Caps,
    present: D3DPRESENT_PARAMETERS,
    indices: Option<IDirect3DIndexBuffer9>,
    transient: Option<IDirect3DVertexBuffer9>,
    transient_capacity: usize,
    transient_ring: Ring,
    dynamic_indices: Option<IDirect3DIndexBuffer9>,
    index_ring: Ring,
    using_dynamic_indices: bool,
    reset_pending: bool,
    lost: bool,
    scene: bool,
    cached_state: Option<(DrawState, usize, usize)>,
    state_cache: StateCache,
    cached_transform: Option<[[f32; 16]; 3]>,
    pub draws: u64,
    pub binds: u64,
    pub uploads: u64,
    pub mesh_uploads: u64,
    pub stream_uploads: u64,
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
        let mut present = D3DPRESENT_PARAMETERS {
            BackBufferWidth: size.width.max(1),
            BackBufferHeight: size.height.max(1),
            BackBufferFormat: backbuffer,
            BackBufferCount: 1,
            MultiSampleType: D3DMULTISAMPLE_NONE,
            SwapEffect: D3DSWAPEFFECT_DISCARD,
            hDeviceWindow: hwnd,
            Windowed: BOOL(1),
            EnableAutoDepthStencil: BOOL(1),
            AutoDepthStencilFormat: depth,
            PresentationInterval: if vsync {
                D3DPRESENT_INTERVAL_ONE as u32
            } else {
                D3DPRESENT_INTERVAL_IMMEDIATE as u32
            },
            ..Default::default()
        };
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
                &mut present,
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
                default_meshes: RefCell::new(Vec::new()),
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
            dynamic_indices: None,
            index_ring: Ring::default(),
            using_dynamic_indices: false,
            reset_pending: false,
            lost: false,
            scene: false,
            cached_state: None,
            state_cache: StateCache::default(),
            cached_transform: None,
            draws: 0,
            binds: 0,
            uploads: 0,
            mesh_uploads: 0,
            stream_uploads: 0,
            index_uploads: 0,
        };
        result.create_indices()?;
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
            dynamic_indices: None,
            index_ring: Ring::default(),
            using_dynamic_indices: false,
            reset_pending: false,
            lost: self.lost,
            scene: false,
            cached_state: None,
            state_cache: StateCache::default(),
            cached_transform: None,
            draws: 0,
            binds: 0,
            uploads: 0,
            mesh_uploads: 0,
            stream_uploads: 0,
            index_uploads: 0,
        }
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
        self.present.BackBufferWidth = width;
        self.present.BackBufferHeight = height;
        self.reset_pending = true;
    }
    pub fn vsync(&mut self, on: bool) {
        let interval = if on {
            D3DPRESENT_INTERVAL_ONE as u32
        } else {
            D3DPRESENT_INTERVAL_IMMEDIATE as u32
        };
        if self.present.PresentationInterval != interval {
            self.present.PresentationInterval = interval;
            self.reset_pending = true;
        }
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
            self.dynamic_indices = None;
            self.index_ring.reset();
            // SAFETY: no outstanding lock, scene, stateblock or backbuffer reference exists.
            unsafe {
                self.inner.device.SetStreamSource(0, None, 0, 0)?;
                self.inner.device.SetIndices(None)?;
                self.inner.device.SetTexture(0, None)?;
                self.inner.device.SetTexture(1, None)?;
                match self.inner.device.Reset(&mut self.present) {
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
        }
        self.using_dynamic_indices = false;
        self.mesh_uploads = 0;
        self.stream_uploads = 0;
        self.index_uploads = 0;
        self.draws = 0;
        self.binds = 0;
        self.cached_state = None;
        self.state_cache = StateCache::default();
        self.cached_transform = None;
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
    pub fn clear_depth(&self) -> Result<()> {
        // SAFETY: live auto depth surface; reverse-Z clears to far (0).
        unsafe {
            self.inner
                .device
                .Clear(0, ptr::null(), D3DCLEAR_ZBUFFER as u32, 0, 0.0, 0)?;
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
                .device
                .Present(ptr::null(), ptr::null(), HWND::default(), ptr::null())
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
        // A pair of Z reflections preserves every clip-space coordinate while
        // making D3D fixed-function camera-space Z positive for linear vertex fog.
        for index in [2, 6, 10, 14] {
            view[index] = -view[index];
        }
        for index in [8, 9, 10, 11] {
            projection[index] = -projection[index];
        }
        let previous = self.cached_transform;
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
            return Err("D3D9 mesh must contain complete quads within the caps/index limit".into());
        }
        let bytes = bytemuck::cast_slice::<_, u8>(vertices);
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
                FVF,
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
                vertices: vertices.len() as u32,
                filled: vertices.len() as u32,
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
            if !self.using_dynamic_indices {
                self.inner.device.SetIndices(indices)?;
                self.using_dynamic_indices = true;
            }
            self.inner
                .device
                .SetStreamSource(0, raw, 0, size_of::<Vertex>() as u32)?;
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
            cache.render(d, D3DRS_FOGENABLE, u32::from(state.fog.is_some()))?;
            if let Some((color, start, end)) = state.fog {
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
                        D3DTEXF_POINT.0
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
        self.shared_indices()?;
        let raw = mesh.raw.borrow();
        let raw = raw
            .as_ref()
            .ok_or("D3D9 terrain buffer needs re-upload after Reset")?;
        // SAFETY: range checked above; shared indices start at zero relative to the base vertex.
        unsafe {
            self.inner
                .device
                .SetStreamSource(0, raw, 0, size_of::<Vertex>() as u32)?;
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
        let limit = self.max_vertices() / primitive * primitive;
        for chunk in vertices.chunks(limit) {
            if chunk.is_empty() {
                continue;
            }
            let bytes = bytemuck::cast_slice::<_, u8>(chunk);
            if self.transient.is_none() {
                self.transient = None;
                let mut buffer = None;
                self.transient_capacity = self.max_vertices() * size_of::<Vertex>();
                self.transient_ring.reset();
                // SAFETY: capacity is bounded by max_vertices; DEFAULT is released before Reset.
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
            if kind == 0 {
                self.shared_indices()?;
            }
            let (offset, discard) = self
                .transient_ring
                .reserve(bytes.len(), self.transient_capacity)
                .ok_or("D3D9 transient ring overflow")?;
            let base = (offset / size_of::<Vertex>()) as u32;
            let buffer = self
                .transient
                .as_ref()
                .ok_or("D3D9 dynamic vertex buffer missing")?;
            // SAFETY: matching lock size/copy length. DISCARD allows the driver to rename in-flight storage.
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
                ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
                buffer.Unlock()?;
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
            self.uploads += bytes.len() as u64;
            self.stream_uploads += bytes.len() as u64;
        }
        Ok(())
    }
    /// Capture only on request. Readback is deliberately absent from the normal frame path.
    pub fn screenshot(&self) -> Result<(u32, u32, Vec<u8>)> {
        // SAFETY: temporary backbuffer and SYSTEMMEM surfaces are released before returning/Reset.
        unsafe {
            let source = self
                .inner
                .device
                .GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO)?;
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
