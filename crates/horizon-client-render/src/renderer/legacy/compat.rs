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
        [
            self.arenas
                .values()
                .map(|a| u64::from(a.live) * a.mesh.stride() as u64)
                .sum(),
            0,
        ]
    }
    pub fn section_vertex_bytes(&self) -> [u64; 2] {
        [self.gpu_mesh_bytes, self.vertex_bytes_split()[0]]
    }
    pub fn gpu_census(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("D3D9 scaled color/depth", self.device.world_target_bytes()),
            ("D3D9 glint textures", self.glint.bytes()),
            (
                "D3D9 terrain buffers (caps-selected pool)",
                self.gpu_mesh_bytes,
            ),
            (
                "D3D9 static block entity VB (system memory)",
                self.static_entities.vertex_bytes(),
            ),
            (
                "D3D9 cached visibility indices (caps-selected pool)",
                self.solid_batches
                    .values()
                    .map(mesh::DrawBatch::gpu_bytes)
                    .sum::<u64>(),
            ),
            (
                "D3D9 sprite pages (managed)",
                self.sprites.pages.iter().map(|p| p.bytes).sum(),
            ),
            ("D3D9 GUI pages (managed)", self.hud.atlas.bytes()),
        ]
    }
    pub fn gpu_memory_by_label(&self) -> Option<gfx::GpuMemory> {
        None
    }
    pub fn cpu_census(&self) -> [(&'static str, usize); 7] {
        [
            (
                "D3D9 section metadata",
                self.sections.heap_bytes()
                    + self
                        .sections
                        .values()
                        .map(|s| {
                            size_of::<dx::MeshData>()
                                + 2 * size_of::<usize>()
                                + s.solid.capacity() * size_of::<mesh::Packet>()
                                + s.translucent.capacity() * size_of::<dx::TextureRange>()
                                + s.gpu.capacity() * size_of::<mesh::GpuSlice>()
                                + s.translucent_order.borrow().bytes()
                        })
                        .sum::<usize>(),
            ),
            (
                "D3D9 GUI CPU cache/scratch",
                self.hud.sprite_animations.capacity()
                    * size_of::<(Rect, SpriteAnimation, (usize, usize, u32))>()
                    + self.hud.atlas.cpu_bytes()
                    + self.panorama.as_ref().map_or(0, Pixels::cpu_bytes)
                    + self.pictures.values().map(Pixels::cpu_bytes).sum::<usize>()
                    + self.gui_batches.bytes()
                    + self.gui_vertices.capacity() * size_of::<dx::Vertex>()
                    + self.gui_screen.capacity() * size_of::<dx::ScreenVertex>()
                    + self.gui_commands.capacity() * size_of::<frame::GuiCommand>()
                    + self.gui_cache.bytes()
                    + self.gui_depth.bytes(),
            ),
            ("D3D9 retained CPU geometry", self.mesh_bytes as usize),
            (
                "D3D9 entity staging scratch",
                self.entity_stage_vertices.capacity() * size_of::<dx::Vertex>(),
            ),
            (
                "D3D9 static block entity buffers and ranges",
                self.static_entities.cpu_bytes(),
            ),
            ("D3D9 animation cache/scratch", self.sprites.cpu_bytes()),
            (
                "D3D9 index/mesh scratch",
                self.decode_scratch.capacity() * size_of::<dx::Vertex>()
                    + self.convert_scratch.bytes()
                    + self
                        .solid_batches
                        .values()
                        .map(mesh::DrawBatch::bytes)
                        .sum::<usize>()
                    + self.index_scratch.bytes()
                    + self.solid_cache.bytes(),
            ),
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
    pub fn start_occlusion(&mut self, camera: DVec3, distance: f32) {
        self.occlusion.update(camera, (distance / 16.0) as i32 + 2);
    }
    pub fn section_occlusion_visible(
        &self,
        key: SectionKey,
        camera_section: SectionKey,
    ) -> Option<bool> {
        self.occlusion.visible_for_camera(key, camera_section)
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
        self.sections.keys().map(|(x, _, z)| (x, z))
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
    pub fn terrain_looks(&self) -> bool {
        false
    } // No shader material-map vertex payload.
    pub fn queue_upload_retries(
        &mut self,
        _dirty: &mut crate::world::DirtySections,
        _now: Instant,
    ) -> usize {
        0
    } // Retained CPU shadows already retry residency.
    pub fn stage_prepared_draws(&mut self, _draws: &[gfx::PreparedModelDraw]) {} // prepared_generation is None: CPU fallback is used.
    pub fn take_world(&mut self, _from: &mut Self) -> bool {
        false
    } // Atlas UVs are baked; resource reloads remesh.
    pub fn translucent_cracks_transparent(&self) -> bool {
        false
    }
    pub fn native_pack_name(&self) -> Option<&str> {
        None
    }
    pub fn release_world(&mut self) {} // No shader pack holds the direct world path.
    pub fn set_world_view_own_level(&mut self, _level: Option<mod_api::LevelRef>) {}
    pub fn prepared_generation(&self) -> Option<u64> {
        None
    }
    pub fn prepared_instance_room(&self, _already: u64, _incoming: usize) -> usize {
        0
    }
    pub fn draws_entity_stage(&self) -> bool {
        false
    }
    pub fn names_entity_runs(&self) -> bool {
        false
    }
    pub fn traces_entities(&self) -> bool {
        false
    }
    pub fn ray_entity_reach(&self, _camera: DVec3) -> Option<gfx::Frustum> {
        None
    }
    pub fn take_entity_runs(
        &mut self,
        runs: &mut gfx::native_entity::EntityRuns,
        posed: &mut gfx::native_entity::EntityRuns,
    ) {
        runs.clear();
        posed.clear();
    }
    pub fn take_ray_only(
        &mut self,
        vertices: &mut Vec<Vertex>,
        runs: &mut gfx::native_entity::EntityRuns,
    ) {
        vertices.clear();
        runs.clear();
    }
    pub fn build_mod_pipelines(
        &mut self,
        set: &crate::mod_pipelines::Pipelines,
        _sprite: &dyn Fn(&str) -> Option<[f32; 4]>,
    ) -> std::result::Result<(), String> {
        if set.built().iter().any(Option::is_some) {
            Err("Custom GPU pipelines are unavailable on Direct3D 9".into())
        } else {
            Ok(())
        }
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
        self.world_scale = if scale.is_finite() {
            scale.clamp(0.25, 2.0)
        } else {
            1.0
        };
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
            (block_light::LightTable, ray_lights::Sources),
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
    pub fn set_pass_detail(&mut self, on: bool) {}
    pub fn set_keep_timed_frames(&mut self, on: bool) {}
    pub fn take_timed_frames(&mut self) -> gfx::pack_timing::TimedFrames {
        Default::default()
    }
    pub fn gpu_readout(&self) -> Option<gfx::pack_timing::GpuReadout> {
        None
    }
    pub fn pass_detail(&self) -> bool {
        false
    }
    pub fn pass_detail_support(&self) -> Option<gfx::pack_timing::DetailSupport> {
        None
    }
    pub fn own_pass_parts(&self) -> Vec<(String, Vec<(String, f32)>)> {
        Vec::new()
    }
    pub fn own_pass_counters(&self) -> Vec<(String, gfx::pack_timing::PassCounters)> {
        Vec::new()
    }
    pub fn set_world_text_shaders(&mut self, shaders: &gfx::core_text::TextShaders) {}
    pub fn set_pass_timing_gap(&mut self, gap: std::time::Duration) {}
    pub fn shader_pass_times(&self) -> Vec<(String, f32)> {
        Vec::new()
    }
    pub fn own_pass_times(&self) -> Vec<(String, f32)> {
        Vec::new()
    }
    pub fn cpu_pass_times(&self) -> Vec<(String, f32)> {
        self.stage_times
            .iter()
            .filter(|(name, _)| name != "D3D9 CPU frame")
            .cloned()
            .collect()
    }
    pub fn define_pack_macros(&self, macros: &mut horizon_shaders::Macros) {}
    pub fn shader_pack_dimension(&self) -> Option<i32> {
        None
    }
    pub fn set_texture_filtering(&mut self, filtering: i32, bit: i32) {
        self.linear = filtering > 0;
        self.device.set_anisotropy(if filtering == 2 {
            1 << bit.clamp(0, 4)
        } else {
            1
        });
    }
    pub fn sprite_split(&self) -> u32 {
        0
    }
    pub fn set_mipmap_levels(&mut self, levels: u32) {
        if let Err(error) = self.sprites.set_mipmap_levels(&mut self.device, levels) {
            log::warn!(
                "D3D9 mipmap-level change could not allocate its textures: {error}; keeping available levels and the saved option"
            );
        }
        self.mipmaps = self.sprites.pages.iter().any(|p| p.levels > 1);
    }
    /// DX9 LOST/NOTRESET is recovered in begin(), without replacing the gameplay renderer.
    pub fn device_lost(&self) -> bool {
        false
    }
    pub fn not_drawn(&self) -> Option<NotDrawn> {
        if self.suspended {
            Some(NotDrawn::Suspended)
        } else {
            self.device.is_lost().then_some(NotDrawn::Lost)
        }
    }
    pub fn mark_device_lost(&self, marker: &std::path::Path) -> bool {
        false
    }
    pub fn msaa_samples(&self) -> u32 {
        1
    }
    pub fn msaa_requested(&self) -> bool {
        self.msaa_requested
    }
    pub fn set_msaa4x(&mut self, requested: bool) {
        self.msaa_requested = requested;
    }
    pub fn set_msaa_rebuildable(&mut self, rebuildable: bool) {}
    pub fn take_msaa_rebuild(&mut self) -> bool {
        false
    }
    pub fn ray_queries(&self) -> bool {
        false
    }
    pub fn ray_looks(&self) -> bool {
        false
    }
    pub fn screenshot_in_flight(&self) -> bool {
        self.screenshot.is_some() || self.screenshot_result.is_some()
    }
    pub fn supports_frame_export(&self) -> bool {
        false
    }
    pub fn begin_frame_export(
        &mut self,
        size: [u32; 2],
        out: std::sync::mpsc::SyncSender<gfx::ExportFrame>,
    ) -> Result<()> {
        Err("Frame export is unavailable on Direct3D9".into())
    }
    pub fn export_adapter_name(&self) -> String {
        self.info.name.clone()
    }
    pub fn try_arm_export_frame(&mut self, index: u32) -> Result<bool> {
        Err("No frame export".into())
    }
    pub fn disarm_export_frame(&mut self) -> bool {
        false
    }
    pub fn frame_export_failed(&self) -> bool {
        false
    }
    pub fn frame_export_recycler(&self) -> Option<std::sync::mpsc::SyncSender<Vec<u8>>> {
        None
    }
    pub fn flush_frame_export_uploads(&mut self) {}
    pub fn begin_finish_frame_export(&mut self) {}
    pub fn poll_finish_frame_export(&mut self) -> Result<bool> {
        Ok(true)
    }
    pub fn abort_frame_export(&mut self) -> Result<()> {
        Ok(())
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
