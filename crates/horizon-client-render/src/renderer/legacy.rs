//! Fixed-function DX9 consumes the same Frame/SectionMesh/HudBatch as gfx.
//! wgpu types below are CPU metadata only; this module never creates a wgpu device.
use crate::{
    Result,
    block_entity_render::{StaticCacheFrameStats, StaticSectionRevision},
    chunk::ColumnLayout,
    gfx::{self, *},
    hud::Rect,
    mesher::{SectionMesh, TerrainVertex, Vertex},
    sprites::SpriteAnimation,
};
use glam::{DVec3, Mat4, Vec3};
use horizon_client_d3d9 as dx;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::window::Window;
mod atlas;
mod compat;
mod frame;
mod mesh;
use atlas::{Pixels, Sprites};
use mesh::Section;

pub struct Hud {
    pub sprite_animations: Vec<(Rect, SpriteAnimation, (usize, usize, u32))>,
    atlas: Pixels,
}
pub struct Renderer {
    device: dx::Device,
    sprites: Sprites,
    lightmap: dx::Texture,
    panorama: Option<Pixels>,
    panorama_faces: Arc<panorama::Faces>,
    pictures: HashMap<u64, Pixels>,
    lightning_cache: HashMap<i64, (Vec<Vertex>, [f32; 2])>,
    white: dx::Texture,
    forcefield: dx::Texture,
    pub hud: Hud,
    pub config: wgpu::SurfaceConfiguration,
    pub info: wgpu::AdapterInfo,
    pub suspended: bool,
    pub paced_elsewhere: bool,
    pub gpu_times: Vec<f32>,
    pub lightmap_pixels: [u8; 1024],
    pub last_wait: Duration,
    pub last_encode: Duration,
    pub multi_draw: bool,
    pub separate_ao: bool,
    pub drawn_sections: usize,
    pub section_draws: usize,
    pub section_draw_calls: usize,
    pub uploaded_bytes: u64,
    pub screenshot: Option<ScreenshotRequest>,
    pub screenshot_result: Option<std::result::Result<[u32; 2], String>>,
    pub screenshot_hdr: Option<std::path::PathBuf>,
    pub saved_screenshots: Vec<SavedScreenshot>,
    pub screenshot_quality: u32,
    pub gamma: f32,
    pub fullbright: bool,
    pub improved_transparency: bool,
    pub animated_textures: bool,
    pub capture: Option<capture::Request>,
    pub capture_result: Option<std::result::Result<serde_json::Value, String>>,
    sections: HashMap<SectionKey, Section>,
    layout: ColumnLayout,
    revision: u64,
    mesh_bytes: u64,
    mesh_budget: u64,
    texture_budget: u64,
    pending_error: Option<String>,
    gpu_mesh_bytes: u64,
    upload_cpu: Duration,
    timings: Vec<(&'static str, f32)>,
    texture_target_warned: bool,
    cache_warning: bool,
    dropped: bool,
    sides: bool,
    linear: bool,
    mipmaps: bool,
    custom_sky_draws: Vec<crate::custom_sky::Draw>,
    last_log: Instant,
    frames: u64,
    stage_times: Vec<(String, f32)>,
    hold: WorldHold,
    shader_refusal: Option<(i32, String, Instant)>,
}
impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        _offscreen: bool,
        sprite_pixels: &[u8],
        sprite_size: u32,
        _sprite_minimum_size: u32,
        layers: u32,
        sprite_mips: &[crate::mipmap::Settings],
        animations: &[SpriteAnimation],
        hud_atlas: (&[u8], u32, u32),
        _glint: (&[u8], u32, u32),
        _glint_armor: (&[u8], u32, u32),
        forcefield: (&[u8], u32),
        panorama_faces: Arc<panorama::Faces>,
        _prepared: Option<PreparedTextures>,
        previous: Option<&Self>,
        _api: crate::graphics::GraphicsApi,
        _translucent_batching: bool,
        _shader_pack: bool,
        _settings: &crate::graphics::Graphics,
        _request: output::FrameRequest,
        _display: output::DisplayReading,
    ) -> Result<Self> {
        let size = window.inner_size();
        let mut device = match previous {
            Some(r) => r.device.reuse(),
            None => dx::Device::new(window, true)?,
        };
        let advisory = u64::from(device.caps.available_texture_bytes);
        let default_budget = if advisory == 0 {
            64 << 20
        } else {
            (advisory / 3).clamp(16 << 20, 128 << 20)
        };
        let mesh_budget = budget("HORIZON_D3D9_MESH_MIB", default_budget)?;
        let texture_budget = budget("HORIZON_D3D9_TEXTURE_MIB", default_budget)?;
        let sprites = Sprites::new(
            &mut device,
            sprite_pixels,
            sprite_size,
            layers,
            sprite_mips,
            animations,
        )?;
        let atlas = Pixels::new(
            &mut device,
            hud_atlas.0,
            hud_atlas.1,
            hud_atlas.2,
            texture_budget / 4,
        )?;
        let white = device.texture(1, 1, 1, &[255; 4])?;
        let forcefield = device.texture(forcefield.1, forcefield.1, 1, forcefield.0)?;
        let lightmap = device.texture(16, 16, 1, &[255; 1024])?;
        let texture_bytes = sprites.layout.bytes() + atlas.bytes() + lightmap.bytes;
        if texture_bytes > texture_budget {
            log::warn!(
                "D3D9 textures exceed the advisory target ({} / {} MiB); the driver's managed residency will page them as needed",
                texture_bytes >> 20,
                texture_budget >> 20
            );
        }
        crate::mesher::packed::pack_quads(false);
        crate::mesher::group_parts(false);
        log::info!(
            "Direct3D9 legacy: fixed-function HAL; no shader compiler, hardware VS requirement, wgpu device or GPU compute. mesh_budget={} texture_budget={}",
            mesh_budget,
            texture_budget
        );
        log::warn!(
            "Direct3D9 limitations: SDR; no shader packs, distant LOD, GPU occlusion/skinning, HDR, TAA/FXAA, frame generation, PBR, post chains or fancy clouds. Stored graphics options remain unchanged."
        );
        let info = wgpu::AdapterInfo {
            name: format!("Direct3D9 HAL: {}", device.caps.name),
            vendor: device.caps.vendor,
            device: device.caps.device,
            device_type: wgpu::DeviceType::Other,
            driver: "Direct3D9 HAL".into(),
            driver_info: format!(
                "VP={}",
                if device.caps.software_vertices {
                    "software"
                } else {
                    "hardware"
                }
            ),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::Other, wgpu::Backend::Noop)
        };
        Ok(Self {
            device,
            sprites,
            lightmap,
            panorama: None,
            panorama_faces,
            pictures: HashMap::new(),
            lightning_cache: HashMap::new(),
            white,
            forcefield,
            hud: Hud {
                atlas,
                sprite_animations: Vec::new(),
            },
            config: wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Bgra8Unorm,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                color_space: wgpu::SurfaceColorSpace::default(),
                view_formats: Vec::new(),
            },
            info,
            suspended: false,
            paced_elsewhere: false,
            gpu_times: Vec::new(),
            lightmap_pixels: [0; 1024],
            last_wait: Duration::ZERO,
            last_encode: Duration::ZERO,
            multi_draw: false,
            separate_ao: false,
            drawn_sections: 0,
            section_draws: 0,
            section_draw_calls: 0,
            uploaded_bytes: 0,
            screenshot: None,
            screenshot_result: None,
            screenshot_hdr: None,
            saved_screenshots: Vec::new(),
            screenshot_quality: 1,
            gamma: 0.0,
            fullbright: false,
            improved_transparency: false,
            animated_textures: true,
            capture: None,
            capture_result: None,
            sections: HashMap::new(),
            layout: ColumnLayout::default(),
            revision: 0,
            mesh_bytes: 0,
            mesh_budget,
            texture_budget,
            pending_error: None,
            gpu_mesh_bytes: 0,
            upload_cpu: Duration::ZERO,
            timings: Vec::new(),
            texture_target_warned: false,
            cache_warning: false,
            dropped: false,
            sides: true,
            linear: false,
            mipmaps: true,
            custom_sky_draws: Vec::new(),
            last_log: Instant::now(),
            frames: 0,
            stage_times: Vec::new(),
            hold: WorldHold::default(),
            shader_refusal: None,
        })
    }
}
fn budget(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Err(_) => Ok(default),
        Ok(s) => {
            let mib: u64 = s
                .parse()
                .map_err(|_| format!("{name} must be an integer MiB budget"))?;
            if !(4..=1024).contains(&mib) {
                return Err(format!("{name} must be between 4 and 1024 MiB").into());
            }
            Ok(mib << 20)
        }
    }
}
fn light_uv(light: u32) -> [f32; 2] {
    [
        ((light >> 8 & 255) as f32 / 256.0 + 0.5 / 16.0).min(15.5 / 16.0),
        ((light & 255) as f32 / 256.0 + 0.5 / 16.0).min(15.5 / 16.0),
    ]
}
fn vertex(v: &Vertex) -> dx::Vertex {
    dx::Vertex {
        pos: v.pos,
        uv: v.uv,
        color: dx::rgba_to_argb(v.color),
        light: light_uv(v.light),
    }
}
