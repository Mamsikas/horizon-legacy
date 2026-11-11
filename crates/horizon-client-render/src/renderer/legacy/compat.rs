//! Explicitly unavailable GPU effects. These adapters never rewrite Graphics or Options.
use super::*;
#[allow(unused_variables)]
impl Renderer {
    pub fn lod_quads(&self) -> u64 {
        0
    }
    pub fn lod_levels(&self) -> Vec<[u64; 2]> {
        Vec::new()
    }
    pub fn packed_layout(&self) -> [bool; 2] {
        [false; 2]
    }
    pub fn vertex_bytes_split(&self) -> [u64; 2] {
        self.section_vertex_bytes()
    }
    pub fn section_vertex_bytes(&self) -> [u64; 2] {
        [
            self.sections
                .values()
                .flat_map(|s| &s.solid)
                .map(|p| u64::from(p.count) * size_of::<dx::Vertex>() as u64)
                .sum(),
            self.sections
                .values()
                .flat_map(|s| &s.translucent)
                .map(|p| u64::from(p.count) * size_of::<dx::Vertex>() as u64)
                .sum(),
        ]
    }
    pub fn gpu_census(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("D3D9 section VBs (DEFAULT cache)", self.gpu_mesh_bytes),
            ("D3D9 sprite pages (managed)", self.sprites.layout.bytes()),
            ("D3D9 GUI pages (managed)", self.hud.atlas.bytes()),
        ]
    }
    pub fn gpu_memory_by_label(&self) -> Option<(u64, u64, Vec<(String, u64)>)> {
        None
    }
    pub fn cpu_census(&self) -> [(&'static str, usize); 4] {
        [
            (
                "D3D9 section metadata",
                self.sections.capacity() * size_of::<Section>(),
            ),
            (
                "D3D9 GUI animation metadata",
                self.hud.sprite_animations.capacity()
                    * size_of::<(Rect, SpriteAnimation, (usize, usize, u32))>(),
            ),
            ("D3D9 retained CPU geometry", self.mesh_bytes as usize),
            ("D3D9 textures (managed backing counted by driver)", 0),
        ]
    }
    pub fn set_shared_material_maps(&mut self, maps: &[Option<crate::paged_maps::PagedMap>; 2]) {}
    pub fn set_custom_sky(&mut self, pack: &dyn crate::custom_sky::SkyLayers) {}
    pub fn custom_sky_draws(&mut self) -> &mut Vec<crate::custom_sky::Draw> {
        &mut self.custom_sky_draws
    }
    pub fn set_lines_shader(&mut self, program: Option<&crate::core_shader::CoreProgram>) {}
    pub fn set_sky_shaders(
        &mut self,
        disc: Option<&crate::core_shader::CoreProgram>,
        stars: Option<&crate::core_shader::CoreProgram>,
    ) {
    }
    pub fn release_surface(&mut self) {
        self.suspended = true;
        self.clear_sections(self.layout);
        self.dropped = true;
    }
    pub fn recreate_surface(&mut self) -> Result<()> {
        self.suspended = false;
        Ok(())
    }
    pub fn set_present_mode(&mut self, vsync: bool, mode: crate::graphics::PresentMode) {
        self.device.vsync(vsync);
        self.config.present_mode = if vsync {
            wgpu::PresentMode::Fifo
        } else {
            wgpu::PresentMode::Immediate
        };
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.device.resize(width, height);
    }
    pub fn flat_draws(&self) -> bool {
        false
    }
    pub fn set_flat_draws(&mut self, on: bool) -> Result<()> {
        Ok(())
    }
    pub fn wants_water_plane(&self) -> bool {
        false
    }
    pub fn section_sides(&self) -> bool {
        self.sides
    }
    pub fn set_section_sides(&mut self, on: bool) {
        self.sides = on;
    }
    pub fn section_cull(&self) -> bool {
        false
    }
    pub fn set_section_cull(&mut self, on: bool) -> Result<()> {
        Ok(())
    }
    pub fn culled_sections(&self) -> Option<([usize; 2], f32)> {
        None
    }
    pub fn gpu_culling(&self) -> bool {
        false
    }
    pub fn gpu_occluding(&self) -> bool {
        false
    }
    pub fn gpu_frame(&self) -> bool {
        false
    }
    pub fn gpu_fading(&self) -> Option<u32> {
        None
    }
    pub fn set_gpu_culling(&mut self, on: bool) -> Result<()> {
        Ok(())
    }
    pub fn gpu_culling_offer(&self) -> crate::graphics::GpuCullingOffer {
        crate::graphics::GpuCullingOffer::Unsupported
    }
    pub fn has_light_colors(&self) -> bool {
        false
    }
    pub fn light_color(&self, name: &str) -> Option<[f32; 3]> {
        None
    }
    pub fn particle_lights(&self) -> Option<&[horizon_shader_api::manifest::ParticleLight]> {
        None
    }
    pub fn native_shadow(&self) -> bool {
        false
    }
    pub fn pack_shadow_casters(&self) -> (bool, bool) {
        (false, false)
    }
    pub fn pack_entity_shadows(&self) -> bool {
        true
    }
    pub fn start_occlusion(&mut self, camera: DVec3, distance: f32) {}
    pub fn section_occlusion_visible(
        &self,
        key: SectionKey,
        camera_section: SectionKey,
    ) -> Option<bool> {
        None
    }
    pub fn hold_world(&mut self, ask: &WorldHold) {
        self.hold = ask.clone();
    }
    pub fn world_holds_column(&self, column: (i32, i32)) -> bool {
        self.hold.covers((column.0, 0, column.1))
    }
    pub fn shadow_pass_block_entities(
        &self,
        camera: DVec3,
    ) -> impl Iterator<Item = &([i32; 3], u32)> + '_ {
        self.block_entities().filter(|_| false)
    }
    pub fn shadow_pass_drew(&self, pos: [i32; 3]) -> bool {
        false
    }
    pub fn static_block_entity_frame_stats(&self) -> StaticCacheFrameStats {
        StaticCacheFrameStats::default()
    }
    pub fn set_packed_layout(&mut self, on: bool) -> bool {
        crate::mesher::packed::pack_quads(false);
        false
    }
    pub fn take_dropped_sections(&mut self) -> bool {
        std::mem::take(&mut self.dropped)
    }
    pub fn set_terrain_loading(&mut self, loading: bool) {}
    pub fn lod_detail_columns(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.sections.keys().map(|&(x, _, z)| (x, z))
    }
    pub fn lod_detail_revision(&self) -> u64 {
        self.revision
    }
    pub fn set_hdr_calibration(&mut self, calibration: Option<gfx::HdrCalibration>) {}
    pub fn output(&self) -> output::EffectiveOutput {
        output::EffectiveOutput::default()
    }
    pub fn display_reading(&self) -> output::DisplayReading {
        Default::default()
    }
    pub fn hdr_on(&self) -> bool {
        false
    }
    pub fn hdr_line(&self) -> String {
        "Direct3D9 SDR (HDR unavailable)".into()
    }
    pub fn set_hdr_lighting(&mut self, on: bool) {}
    pub fn frame_follows(
        &self,
        settings: &crate::graphics::Graphics,
        request: output::FrameRequest,
    ) -> bool {
        true
    }
    pub fn requested_frame(
        &self,
        settings: &crate::graphics::Graphics,
        request: output::FrameRequest,
    ) -> output::FrameConfiguration {
        output::EffectiveOutput::default().frame
    }
    pub fn measure_emission(&mut self) {}
    pub fn skinned_models(&self) -> Option<u32> {
        None
    }
    pub fn skins_entities(&self) -> bool {
        false
    }
    pub fn skins_tagged(&self) -> bool {
        false
    }
    pub fn frame_generation_share(&self) -> Option<f32> {
        None
    }
    pub fn set_frame_generation(&mut self, on: bool) {}
    pub fn set_world_post(
        &mut self,
        scale: f32,
        antialias: bool,
        taa: bool,
        upscale: f32,
        sharpness: f32,
    ) {
    }
    pub fn set_shader_pack(
        &mut self,
        dimension: i32,
        pack: Option<horizon_shaders::Post>,
        block_ids: &[i32],
    ) -> std::result::Result<Option<(Vec<String>, Vec<String>)>, Refused> {
        if pack.is_some() {
            Err(refused())
        } else {
            Ok(None)
        }
    }
    pub fn reset_shader_pack_history(&mut self) {}
    pub fn shader_pack_kind(&self) -> Option<&'static str> {
        None
    }
    pub fn finish_sequence_frame(&mut self) -> Result<()> {
        Ok(())
    }
    pub fn load_shader_pack(
        &mut self,
        dimension: i32,
        pack: horizon_shaders::Post,
        block_ids: Vec<i32>,
        label: String,
        started: std::time::Instant,
    ) {
        self.shader_refusal = Some((dimension, label, started));
    }
    pub fn poll_shader_pack(
        &mut self,
    ) -> Option<(
        i32,
        String,
        std::time::Instant,
        std::result::Result<Option<(Vec<String>, Vec<String>)>, Refused>,
    )> {
        self.shader_refusal
            .take()
            .map(|(d, l, s)| (d, l, s, Err(refused())))
    }
    pub fn shader_pack_loading(&self) -> bool {
        false
    }
    pub fn drop_shader_packs(&mut self) {}
    pub fn set_separate_ao(&mut self, separate_ao: bool) {}
    pub fn set_pack_lava_layers(&mut self, layers: Vec<u16>) {}
    pub fn set_native_shader_pack(
        &mut self,
        pack: Option<(
            horizon_shader_api::Report,
            Vec<(String, f64)>,
            Vec<(u32, u32)>,
            Vec<(u32, u32)>,
            block_light::LightTable,
        )>,
    ) -> std::result::Result<Vec<String>, String> {
        if pack.is_some() {
            Err("Shader packs are unavailable on Direct3D9".into())
        } else {
            Ok(Vec::new())
        }
    }
    pub fn pack_inputs(&mut self) -> horizon_shaders::GameInputs {
        Default::default()
    }
    pub fn set_pass_timing(&mut self, on: bool) {}
    pub fn set_pass_timing_gap(&mut self, gap: std::time::Duration) {}
    pub fn shader_pass_times(&self) -> Vec<(String, f32)> {
        Vec::new()
    }
    pub fn own_pass_times(&self) -> Vec<(String, f32)> {
        self.stage_times.clone()
    }
    pub fn define_pack_macros(&self, macros: &mut horizon_shaders::Macros) {}
    pub fn shader_pack_dimension(&self) -> Option<i32> {
        None
    }
    pub fn set_texture_filtering(&mut self, filtering: i32, bit: i32) {
        self.linear = filtering > 0;
    }
    pub fn sprite_split(&self) -> u32 {
        0
    }
    pub fn set_mipmap_levels(&mut self, levels: u32) {
        self.mipmaps = levels > 0;
    }
    /// DX9 LOST/NOTRESET is recovered in begin(), without replacing the gameplay renderer.
    pub fn device_lost(&self) -> bool {
        false
    }
    pub fn stats_lag(&self) -> u64 {
        0
    }
    pub fn wait_in_flight(&mut self) {}
    pub fn wait_submissions(&mut self) {}
}
fn refused() -> Refused {
    Refused {
        reason: "Shader packs are unavailable on Direct3D9".into(),
        notes: vec![],
    }
}

#[derive(Debug)]
pub struct Refused {
    pub reason: String,
    pub notes: Vec<String>,
}
impl Refused {
    pub fn is_cancelled(&self) -> bool {
        false
    }
}
impl Renderer {
    pub fn max_texture_dimension(&self) -> u32 {
        self.device.caps.max_texture
    }
    pub fn clear_lods(&mut self) {}
    pub fn queue_lods(
        &mut self,
        _meshes: Vec<(crate::lod::TileKey, SectionMesh)>,
        _removed: Vec<crate::lod::TileKey>,
        _cutout: Option<crate::fast_hash::FxHashSet<(i32, i32)>>,
    ) {
    }
    pub fn upload_lods(&mut self, _budget: Duration) {}
    pub fn set_lod_cutout(&mut self, _cutout: crate::fast_hash::FxHashSet<(i32, i32)>) {}
}
impl Renderer {
    pub fn keeps_capture_copies(&self) -> bool {
        false
    }
    pub fn keep_capture_copies(&mut self, _on: bool) {
        log::warn!(
            "Direct3D9 CPU render captures are unavailable; use screenshots and the D3D9 counters"
        );
    }
    pub fn hdr_driver(&self) -> Option<&str> {
        None
    }
    pub fn drop_hdr_marker(&mut self) {}
    pub fn display_changed(&mut self) -> bool {
        false
    }
    pub fn update_output(
        &mut self,
        _settings: &crate::graphics::Graphics,
        _marker: &std::path::Path,
    ) {
    }
    pub fn set_animation_inputs(
        &mut self,
        _day_time: i64,
        _weather: [f32; 2],
        _frame: Option<&crate::environment::Frame>,
        _screen: Option<u64>,
    ) {
    }
    pub fn compositor(&self, _window: Arc<Window>) -> Result<gfx::compositor::Compositor> {
        Err("Split-screen composition is unavailable in this Direct3D9 build".into())
    }
    pub fn offscreen_frame(&self) -> Option<&wgpu::Texture> {
        None
    }
    pub fn take_back(&self, _compositor: gfx::compositor::Compositor) {}
}
