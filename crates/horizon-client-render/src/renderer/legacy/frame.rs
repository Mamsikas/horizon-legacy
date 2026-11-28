use super::*;
use dx::{Blend, DrawState};
pub(super) mod gui_cache;
pub(super) mod static_entities;

pub(super) struct Batch {
    pub(super) page: usize,
    pub(super) state: DrawState,
    pub(super) vertices: Vec<dx::Vertex>,
    beam_v: Option<[f32; 2]>,
}
struct EntityBatches {
    pixels: Vec<Batch>,
    sprites: Vec<Batch>,
    glints: Vec<Batch>,
}
// Wait for a pass with a material run that repeatedly crosses shared-atlas pages.
// The cost of one extra managed page is unwarranted for a few small draws.
const ENTITY_PAGE_BREAKS: usize = 16;
#[derive(Clone)]
pub(super) enum GuiCommand {
    ClearDepth,
    Draw {
        page: usize,
        state: DrawState,
        picture: Option<u64>,
        range: Range<u32>,
    },
}
pub(super) struct GuiBatches {
    state: DrawState,
    order: dx::PaintWindow,
    ready: Vec<Batch>,
    open: Vec<Batch>,
    pool: Vec<Vec<dx::Vertex>>,
}
impl GuiBatches {
    pub(super) fn new(state: DrawState) -> Self {
        Self {
            state,
            order: dx::PaintWindow::default(),
            ready: Vec::new(),
            open: Vec::new(),
            pool: Vec::new(),
        }
    }
    fn begin(&mut self, state: DrawState) {
        self.state = state;
        self.order.clear();
    }
    pub(super) fn bytes(&self) -> usize {
        self.pool
            .iter()
            .map(|v| v.capacity() * size_of::<dx::Vertex>())
            .sum::<usize>()
            + (self.ready.capacity() + self.open.capacity()) * size_of::<Batch>()
    }
    fn flush(&mut self) {
        self.ready.append(&mut self.open);
    }
    fn push(&mut self, page: usize, poly: &[dx::Vertex]) {
        let bounds = [
            poly.iter().map(|v| v.pos[0]).fold(f32::INFINITY, f32::min),
            poly.iter().map(|v| v.pos[1]).fold(f32::INFINITY, f32::min),
            poly.iter()
                .map(|v| v.pos[0])
                .fold(f32::NEG_INFINITY, f32::max),
            poly.iter()
                .map(|v| v.pos[1])
                .fold(f32::NEG_INFINITY, f32::max),
        ];
        if self.order.barrier(page, bounds) {
            self.flush();
        }
        let at = if let Some(at) = self.open.iter().position(|b| b.page == page) {
            at
        } else {
            self.open.push(Batch {
                page,
                state: self.state,
                vertices: self.pool.pop().unwrap_or_default(),
                beam_v: None,
            });
            self.open.len() - 1
        };
        atlas::triangles(poly, &mut self.open[at].vertices);
    }
    fn finish(
        &mut self,
        vertices: &mut Vec<dx::Vertex>,
        commands: &mut Vec<GuiCommand>,
        picture: Option<u64>,
    ) {
        self.flush();
        for mut batch in self.ready.drain(..) {
            let first = vertices.len() as u32;
            vertices.extend_from_slice(&batch.vertices);
            commands.push(GuiCommand::Draw {
                page: batch.page,
                state: batch.state,
                picture,
                range: first..vertices.len() as u32,
            });
            batch.vertices.clear();
            self.pool.push(batch.vertices);
        }
    }
}
fn batch(
    out: &mut Vec<Batch>,
    page: usize,
    state: DrawState,
    poly: &[dx::Vertex],
    as_triangles: bool,
) {
    if out
        .last()
        .is_none_or(|b| b.page != page || b.state != state || b.beam_v.is_some())
    {
        out.push(Batch {
            page,
            state,
            vertices: Vec::new(),
            beam_v: None,
        });
    }
    if let Some(b) = out.last_mut() {
        if as_triangles {
            atlas::triangles(poly, &mut b.vertices);
        } else {
            atlas::quads(poly, &mut b.vertices);
        }
    }
}
fn draw_pixels(
    device: &mut dx::Device,
    pixels: atlas::PixelView<'_>,
    lightmap: &dx::Texture,
    batches: &[Batch],
    triangles: bool,
) -> Result<()> {
    pixels.prepare(device)?;
    let mut textures = pixels.draw_textures();
    for b in batches {
        if let Some([origin, height]) = b.beam_v {
            drop(textures);
            let quad = b.vertices.as_slice().try_into()?;
            let mut strips = Vec::new();
            let mut bands = 0;
            beam_bands(quad, 1, |poly, band| {
                for vertex in poly.iter_mut() {
                    vertex.uv[1] = origin + (vertex.uv[1] - band) * height;
                }
                pixels.polygon(poly, |page, p| batch(&mut strips, page, b.state, p, false));
                bands += 1;
                if bands == 16 {
                    // Bound scratch space, keeping these strips in the original draw order.
                    draw_pixels(device, pixels, lightmap, &strips, false)?;
                    strips.clear();
                    bands = 0;
                }
                Ok(())
            })?;
            if !strips.is_empty() {
                draw_pixels(device, pixels, lightmap, &strips, false)?;
            }
            textures = pixels.draw_textures();
            continue;
        }
        let Some(texture) = textures.get(b.page) else {
            continue;
        };
        device.state(b.state, texture, Some(lightmap))?;
        if triangles {
            device.draw_triangles(&b.vertices)?;
        } else {
            device.draw_transient(&b.vertices)?;
        }
    }
    Ok(())
}
impl Renderer {
    pub fn views(&self, frame: &Frame) -> ViewSet {
        ViewSet::single(View::screen(
            frame.yaw,
            frame.pitch,
            frame.fov_degrees,
            self.config.width as f32 / self.config.height.max(1) as f32,
            frame.depth_far,
        ))
    }
    fn local_camera_frustum(
        &self,
        fov_degrees: f32,
        yaw: f32,
        pitch: f32,
        bob: Mat4,
        depth_far: f32,
    ) -> Frustum {
        Frustum::from_view_projection(
            View::screen(
                yaw,
                pitch,
                fov_degrees,
                self.config.width as f32 / self.config.height.max(1) as f32,
                depth_far,
            )
            .view_proj(bob),
        )
    }
    pub fn camera_frustum(
        &self,
        _camera: DVec3,
        fov_degrees: f32,
        yaw: f32,
        pitch: f32,
        bob: Mat4,
        depth_far: f32,
    ) -> gfx::JoinedFrusta {
        self.local_camera_frustum(fov_degrees, yaw, pitch, bob, depth_far)
            .into()
    }
    pub fn camera_frustum_with_jitter(
        &self,
        fov_degrees: f32,
        yaw: f32,
        pitch: f32,
        bob: Mat4,
        depth_far: f32,
    ) -> gfx::JoinedFrusta {
        self.local_camera_frustum(fov_degrees, yaw, pitch, bob, depth_far)
            .into()
    }
    pub fn render(&mut self, frame: &Frame) -> Result<bool> {
        if self.suspended {
            return Ok(false);
        }
        self.drain_uploads()?;
        self.progress_shadows();
        if let Some(error) = self.pending_error.take() {
            return Err(error.into());
        }
        let started = Instant::now();
        self.device.profile_begin_frame();
        self.profile_laps = crate::spans::laps();
        self.timings.clear();
        let clear = if frame.end_sky {
            [0.0; 3]
        } else {
            frame.fog.map_or(frame.sky.fog_color, |f| {
                [f.color[0], f.color[1], f.color[2]]
            })
        };
        match self
            .device
            .begin_scaled(clear, if frame.title { 1.0 } else { self.world_scale })
        {
            Ok(true) => {}
            Ok(false) => {
                self.device.profile_end_frame(false);
                self.last_wait = started.elapsed();
                self.last_encode = Duration::ZERO;
                return Ok(false);
            }
            Err(error) if dx::is_device_lost(error.as_ref()) => {
                self.device.profile_end_frame(false);
                self.device.mark_lost();
                return Ok(false);
            }
            Err(error) => {
                self.device.profile_end_frame(false);
                return Err(error);
            }
        }
        self.timings
            .push(("begin/reset", started.elapsed().as_secs_f32() * 1000.0));
        self.profile_laps.lap("d3d9_begin");
        self.device.mesh_uploads += std::mem::take(&mut self.pending_mesh_uploads);
        if let Err(error) = self.draw_frame(frame) {
            self.device.cancel_frame();
            self.device.profile_end_frame(false);
            if dx::is_device_lost(error.as_ref()) {
                self.device.mark_lost();
                return Ok(false);
            }
            return Err(error);
        }
        self.last_encode = started.elapsed();
        let wait = Instant::now();
        self.device.profile_pass(dx::SubmitPass::Present);
        let finished = self.device.finish();
        self.device.profile_end_frame(matches!(&finished, Ok(true)));
        let shown = finished?;
        self.profile_laps.lap("d3d9_present");
        self.last_wait = wait.elapsed();
        self.uploaded_bytes = std::mem::take(&mut self.device.uploads);
        self.frames += 1;
        self.stage_times.clear();
        self.stage_times.push((
            "D3D9 CPU frame".into(),
            self.last_encode.as_secs_f32() * 1000.0,
        ));
        self.stage_times.extend(
            self.timings
                .iter()
                .map(|(name, ms)| (format!("D3D9 {name}"), *ms)),
        );
        self.stage_times.push((
            "D3D9 section uploads (outside frame)".into(),
            self.upload_cpu.as_secs_f32() * 1000.0,
        ));
        self.stage_times
            .push(("D3D9 Present".into(), self.last_wait.as_secs_f32() * 1000.0));
        if self.diagnostics {
            let cpu_ms = self.last_encode.as_secs_f64() * 1000.0;
            self.profile_window.0 += 1;
            self.profile_window.1 += cpu_ms;
            if cpu_ms >= self.profile_window.2 {
                self.profile_window.2 = cpu_ms;
                self.worst_stages.clone_from(&self.timings);
            }
            if self.last_log.elapsed() >= Duration::from_secs(5) {
                if self.entity_staging {
                    log::info!(
                        "D3D9 entity quad staging groups_total={} fallback_total={} scratch_bytes={}",
                        self.entity_stage_stats[0],
                        self.entity_stage_stats[1],
                        self.entity_stage_vertices.capacity() * size_of::<dx::Vertex>(),
                    );
                    let (tiles, recycled, overflow) = self.hud.atlas.entity_page_stats();
                    log::info!(
                        "D3D9 dynamic entity ranges pixels_total={} sprites_total={} foil_total={} page_breaks_total={} compact_passes_total={} compact_tiles={tiles} recycled_tiles_total={recycled} overflow_tiles_total={overflow}",
                        self.entity_ranges[0],
                        self.entity_ranges[1],
                        self.entity_ranges[2],
                        self.entity_ranges[3],
                        self.entity_ranges[4],
                    );
                }
                self.static_entities.log();
                self.device.log_submit_profile();
                let cpu = dx::thread_cpu_time();
                let busy = cpu
                    .zip(self.last_thread_cpu)
                    .and_then(|(now, old)| now.checked_sub(old))
                    .map(|elapsed| {
                        (elapsed.as_secs_f64() * 100.0 / self.last_log.elapsed().as_secs_f64())
                            .clamp(0.0, 100.0)
                    });
                log::info!(
                    "D3D9 main_thread_busy_pct={busy:?} (CPU execution; driver waits are wall time)"
                );
                self.last_thread_cpu = cpu;
                log::info!(
                    "D3D9 frame={} cpu_ms={:.3} present_ms={:.3} draws={} texture_binds={} uploads={} sections={}/{} mesh_cpu_bytes={} mesh_decoded_bytes={} mesh_gpu_bytes={} arenas={} texture_bytes={} compactions={} eager_sections={}",
                    self.frames,
                    self.last_encode.as_secs_f64() * 1000.0,
                    self.last_wait.as_secs_f64() * 1000.0,
                    self.device.draws,
                    self.device.binds,
                    self.uploaded_bytes,
                    self.drawn_sections,
                    self.sections.len(),
                    self.mesh_bytes,
                    self.decoded_mesh_bytes,
                    self.gpu_mesh_bytes,
                    self.arenas.len(),
                    self.texture_bytes(),
                    self.compactions,
                    self.eager_sections
                );
                log::info!(
                    "D3D9 stages_ms={:?} section_conversion_ms={:.3} process_memory_bytes={:?}",
                    self.timings,
                    self.upload_cpu.as_secs_f64() * 1000.0,
                    horizon_threads::process_memory::usage()
                );
                log::info!(
                    "D3D9 GUI draws={} texture_binds={} vertex_uploads={} buffer_locks={} projected={} depth_clears={} depth_reused={}",
                    self.gui_stats[0],
                    self.gui_stats[1],
                    self.gui_stats[2],
                    self.gui_stats[3],
                    !self.gui_screen.is_empty(),
                    self.gui_depth.clears,
                    self.gui_depth.reused
                );
                log::info!(
                    "D3D9 basic transparency draws={} texture_binds={} page_ranges={}",
                    self.translucent_stats[0],
                    self.translucent_stats[1],
                    self.translucent_stats[2]
                );
                log::info!(
                    "D3D9 fog draws dual={} environment={} distance={} unfogged={} fully_fogged={}",
                    self.device.fog_draws[0],
                    self.device.fog_draws[1],
                    self.device.fog_draws[2],
                    self.device.fog_draws[3],
                    self.device.fog_draws[4]
                );
                log::info!("D3D9 fog parameters={:?}", self.device.fog_parameters());
                log::info!(
                    "D3D9 GUI cache reused_chunks={} rebuilt_chunks={} cpu_bytes={}",
                    self.gui_cache.hits,
                    self.gui_cache.misses,
                    self.gui_cache.bytes()
                );
                log::info!(
                    "D3D9 arrival pages_total={} sections_total={} max_sections_per_page={}",
                    self.arrival_pages,
                    self.arrival_sections,
                    self.arrival_max_sections
                );
                let batches = self
                    .compactor
                    .as_ref()
                    .map_or([0; 2], mesh::Compactor::batch_stats);
                log::info!(
                    "D3D9 staging batches={} max_sections_per_batch={}",
                    batches[0],
                    batches[1]
                );
                if let Some(worker) = &mut self.compactor {
                    worker.log_material_decode();
                }
                log::info!(
                    "D3D9 solid_plan_reused={} solid_culled_buckets={} uncached_visible_bytes={} staging_pending={} staging_reserved_bytes={} staged_sections={}",
                    self.solid_reused,
                    self.solid_cache.culled_buckets(),
                    self.uncached_mesh_bytes,
                    self.pending_sections.len(),
                    self.compactor
                        .as_ref()
                        .map_or(0, mesh::Compactor::reserved_uploads),
                    self.staged_sections
                );
                let mut vertex_bytes = [0u64; 3];
                let mut packed_arenas = 0;
                for a in self.arenas.values() {
                    let i = match a.mesh.stride() {
                        20 => 0,
                        28 => 1,
                        _ => 2,
                    };
                    vertex_bytes[i] += a.mesh.bytes();
                    packed_arenas += usize::from(a.material_packed);
                }
                log::info!(
                    "D3D9 terrain_vertex_bytes compact20={} wide28={} ffp32={} solid_material_packed_arenas={}",
                    vertex_bytes[0],
                    vertex_bytes[1],
                    vertex_bytes[2],
                    packed_arenas
                );
                log::info!(
                    "D3D9 animation_cache hits={} misses={} cpu_bytes={}",
                    self.sprites.frame_cache.hits,
                    self.sprites.frame_cache.misses,
                    self.sprites.cpu_bytes()
                );
                let n = self.stage_profile[0].max(1.0);
                log::info!(
                    "D3D9 staging completed={} queue_wall_avg_ms={:.3} queue_wall_peak_ms={:.3} conversion_wall_avg_ms={:.3} conversion_wall_peak_ms={:.3}",
                    self.stage_profile[0],
                    self.stage_profile[1] / n,
                    self.stage_profile[2],
                    self.stage_profile[3] / n,
                    self.stage_profile[4]
                );
                self.stage_profile = [0.0; 5];
                if let Some(worker) = &self.compactor {
                    let (pending, bytes, active, [count, ns, peak]) = worker.shadow_status();
                    log::info!(
                        "D3D9 deferred shadows pending={} bytes={} active={} completed={} worker_avg_ms={:.3} worker_peak_ms={:.3}",
                        pending,
                        bytes,
                        active,
                        count,
                        ns as f64 / count.max(1) as f64 / 1.0e6,
                        peak as f64 / 1.0e6
                    );
                }
                log::info!(
                    "D3D9 window_frames={} cpu_avg_ms={:.3} cpu_peak_ms={:.3} peak_stages_ms={:?} mesh_uploads={} stream_uploads={} index_uploads={}",
                    self.profile_window.0,
                    self.profile_window.1 / self.profile_window.0 as f64,
                    self.profile_window.2,
                    self.worst_stages,
                    self.device.mesh_uploads,
                    self.device.stream_uploads,
                    self.device.index_uploads
                );
                self.profile_window = (0, 0.0, 0.0);
                self.last_log = Instant::now();
            }
        }

        self.upload_cpu = Duration::ZERO;
        Ok(shown)
    }
    fn stamp(&mut self, name: &'static str, clock: &mut Instant) {
        self.profile_laps.lap(name);
        self.timings
            .push((name, clock.elapsed().as_secs_f32() * 1000.0));
        *clock = Instant::now();
    }
    fn texture_bytes(&self) -> u64 {
        self.sprites.pages.iter().map(|p| p.bytes).sum::<u64>()
            + self.hud.atlas.bytes()
            + self.panorama.as_ref().map_or(0, Pixels::bytes)
            + self.pictures.values().map(Pixels::bytes).sum::<u64>()
            + self.forcefield.bytes
            + self.white.bytes
            + self.lightmap.bytes
    }
    fn draw_frame(&mut self, frame: &Frame) -> Result<()> {
        self.device.profile_pass(dx::SubmitPass::Prepare);
        self.translucent_stats = [0; 3];
        let mut clock = Instant::now();
        self.update_lightmap(frame)?;
        self.stamp("lightmap", &mut clock);
        if self.animated_textures {
            self.sprites.animate(&mut self.device, frame.ticks)?;
        }
        self.stamp("sprite animation", &mut clock);
        for patch in &frame.hud.patches {
            self.hud
                .atlas
                .patch(patch.x, patch.y, patch.width, patch.height, &patch.rgba)?;
        }
        for (rect, animation, old) in &mut self.hud.sprite_animations {
            let selected = animation.frame(frame.ticks);
            if old.0 == selected {
                continue;
            }
            let (x, y) = rect.texels((self.hud.atlas.width, self.hud.atlas.height));
            let pixels = animation.resized_pixels(selected, rect.w, rect.h);
            self.hud.atlas.patch(x, y, rect.w, rect.h, &pixels)?;
            *old = (selected, 0, 0);
        }
        let mut texture_bytes = self.texture_bytes();
        if texture_bytes > self.texture_budget && self.hud.atlas.entity_view().is_some() {
            self.hud.atlas.drop_entity_page();
            texture_bytes = self.texture_bytes();
            log::info!("D3D9 dynamic entity page released to stay within the texture budget");
        }
        if texture_bytes > self.texture_budget && !self.texture_target_warned {
            log::warn!(
                "D3D9 textures exceed the advisory residency target: {} / {} MiB; continuing with managed paging",
                texture_bytes >> 20,
                self.texture_budget >> 20
            );
            self.texture_target_warned = true;
        }
        self.hud.atlas.begin_entity_frame();
        self.stamp("GUI patches", &mut clock);
        let views = self.views(frame);
        let view = views.primary();
        let projection = view.projection * frame.bob * frame.spin;
        let frustum = Frustum::from_view_projection(projection * view.view);
        self.device.transform(
            Mat4::IDENTITY.to_cols_array(),
            view.view.to_cols_array(),
            projection.to_cols_array(),
        )?;
        self.drawn_sections = 0;
        self.section_draws = 0;
        self.section_draw_calls = 0;
        self.solid_reused = false;
        self.uncached_mesh_bytes = 0;
        self.device.profile_pass(dx::SubmitPass::Effects);
        if frame.title {
            self.draw_panorama(frame)?;
        } else {
            self.panorama = None;
            self.draw_sky(frame)?;
            self.device.transform(
                Mat4::IDENTITY.to_cols_array(),
                view.view.to_cols_array(),
                projection.to_cols_array(),
            )?;
            self.draw_end_sky(frame)?;
            self.draw_entities(frame, frame.celestial, None, 2)?;
            let fog = Some(
                frame
                    .fog
                    .map(|f| dx::Fog {
                        color: f.color,
                        environment: [f.environmental_start, f.environmental_end],
                        distance: [f.render_distance_start, f.render_distance_end],
                    })
                    .unwrap_or_else(|| {
                        let (start, end) = frame.environment_fog.unwrap_or((0.0, 1.0e9));
                        dx::Fog {
                            color: [
                                frame.sky.fog_color[0],
                                frame.sky.fog_color[1],
                                frame.sky.fog_color[2],
                                1.0,
                            ],
                            environment: [start, end],
                            distance: [frame.distance * 0.75, frame.distance],
                        }
                    }),
            );
            self.stamp("sky", &mut clock);
            self.device.profile_pass(dx::SubmitPass::Prepare);
            let mut visible = std::mem::take(&mut self.visible);
            visible.clear();
            if frame.cave_culling {
                self.start_occlusion(frame.camera, frame.detail_distance);
                self.prune_hidden(frame.camera);
            }
            self.sections.walk(
                frame.camera,
                (frame.distance / 16.0).ceil() as i32,
                &frustum,
                |key| !frame.cave_culling || self.occlusion.contains(key),
                |_, _| false,
                true,
                &mut visible,
            );
            self.stamp("visibility", &mut clock);
            self.prepare_sections(&visible)?;
            self.stamp("terrain cache", &mut clock);
            self.device.profile_pass(dx::SubmitPass::Solid);
            self.uncached_mesh_bytes = visible
                .iter()
                .map(|v| self.sections.at(v.slot))
                .filter(|s| s.gpu.is_empty())
                .map(|s| s.vertices.len() as u64 * size_of::<dx::Vertex>() as u64)
                .sum();
            #[cfg(feature = "legacy-d3d9-dev")]
            let clip = (self.diagnostics && self.device.caps.software_vertices).then_some(
                solid_cache::Clip {
                    frustum: &frustum,
                    camera: frame.camera,
                    layout: self.layout,
                    clusters: self
                        .device
                        .benchmark_active(dx::benchmark::Trial::SolidClusters),
                },
            );
            #[cfg(not(feature = "legacy-d3d9-dev"))]
            let clip = None;
            self.solid_reused = self.solid_cache.update(
                &self.sections,
                &visible,
                (self.next_arena, self.device.resets),
                self.sides,
                fog,
                clip,
            );
            #[cfg(feature = "legacy-d3d9-dev")]
            if clip.is_some() {
                self.device.profile_solid_clipping(
                    self.solid_cache.culled_buckets(),
                    self.solid_cache.trimmed_vertices(),
                );
            }
            self.drawn_sections = visible.len();
            let mut decoded = std::mem::take(&mut self.decode_scratch);
            let before = self.device.draws;
            if !self.solid_reused {
                self.solid_batches
                    .retain(|(_, _, id, _), _| self.arenas.contains_key(id));
                for batch in self.solid_batches.values_mut() {
                    batch.start_plan();
                }
                for (visible_index, v) in visible.iter().enumerate() {
                    let key = &v.key;
                    let origin = Vec3::from_array(v.origin);
                    let section = self.sections.at(v.slot);
                    let fog_program = solid_cache::section_fog(&self.sections, v, fog);
                    if section.solid.is_empty() {
                        continue;
                    }
                    if section.gpu.is_empty() {
                        section.vertices.decode(&mut decoded)?;
                    }
                    let direct = section.gpu.is_empty();
                    if direct {
                        self.device.transform(
                            Mat4::from_translation(mesh::region_origin(
                                mesh::region_key(*key),
                                self.layout,
                                frame.camera,
                            ))
                            .to_cols_array(),
                            view.view.to_cols_array(),
                            projection.to_cols_array(),
                        )?;
                    }
                    let mut runs = smallvec::SmallVec::<[Range<u32>; 7]>::new();
                    if self.sides {
                        section.sides.visible((-origin).to_array(), |first, count| {
                            runs.push(first..first + count)
                        });
                    } else {
                        runs.push(0..u32::MAX);
                    }
                    let buckets = self.solid_cache.buckets(visible_index);
                    let mut trims = self.solid_cache.packet_trims(v.slot).iter().peekable();
                    for (i, packet) in section.solid.iter().enumerate() {
                        let trim = trims.next_if(|trim| trim.packet == i);
                        if buckets & (1 << packet.bucket) == 0
                            || !runs
                                .iter()
                                .any(|r| r.start < packet.source.end && packet.source.start < r.end)
                        {
                            continue;
                        }
                        if direct {
                            self.device.state(
                                DrawState {
                                    alpha_test: !packet.opaque,
                                    fog,
                                    fog_program,
                                    linear: self.linear,
                                    mipmaps: self.mipmaps,
                                    ..Default::default()
                                },
                                &self.sprites.pages[packet.page],
                                Some(&self.lightmap),
                            )?;
                            mesh::draw_packet(
                                &mut self.device,
                                &self.arenas,
                                section,
                                packet,
                                &decoded,
                            )?;
                        } else {
                            let range = packet.first..packet.first + packet.count;
                            for slice in section.candidate_slices(range.clone()) {
                                let first = range.start.max(slice.source.start);
                                let end = range.end.min(slice.source.end);
                                if first < end {
                                    let batch = self
                                        .solid_batches
                                        .entry((
                                            packet.page,
                                            !packet.opaque,
                                            slice.arena,
                                            fog_program,
                                        ))
                                        .or_default();
                                    let gpu_first = slice.first + first - slice.source.start;
                                    if clip.is_some_and(|clip| clip.clusters) {
                                        let full = gpu_first..gpu_first + end - first;
                                        let keep = trim.map_or_else(
                                            || full.clone(),
                                            |t| {
                                                let start = first.max(t.vertices.start);
                                                let end = end.min(t.vertices.end);
                                                if start < end {
                                                    gpu_first + start - first
                                                        ..gpu_first + end - first
                                                } else {
                                                    0..0
                                                }
                                            },
                                        );
                                        batch.append_clipped(full, keep);
                                    } else {
                                        batch.append(gpu_first, end - first);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            self.decode_scratch = decoded;
            self.stamp("terrain indices/cold draws", &mut clock);
            #[cfg(feature = "legacy-d3d9-dev")]
            let dense = self
                .device
                .benchmark_active(dx::benchmark::Trial::DenseIndices);
            #[cfg(not(feature = "legacy-d3d9-dev"))]
            let dense = false;
            let index_budget = (self.mesh_budget / 16).min(4 << 20);
            let mut cached_bytes = if dense {
                self.solid_batches
                    .values()
                    .map(mesh::DrawBatch::cached_bytes)
                    .sum()
            } else {
                0
            };
            for (&(page, cutout, id, fog_program), batch) in &mut self.solid_batches {
                if dense {
                    cached_bytes -= batch.cached_bytes();
                }
                if batch.next.is_empty() {
                    // A changed fog class must not retain another copy of the
                    // old visible index plan in each inactive program bucket.
                    // Keep small CPU vectors reusable.
                    batch.release_indices();
                    continue;
                }
                let arena = &self.arenas[&id];
                self.device.transform(
                    Mat4::from_translation(mesh::region_origin(
                        arena.region,
                        self.layout,
                        frame.camera,
                    ))
                    .to_cols_array(),
                    view.view.to_cols_array(),
                    projection.to_cols_array(),
                )?;
                self.device.state(
                    DrawState {
                        alpha_test: cutout,
                        fog,
                        fog_program,
                        linear: self.linear,
                        mipmaps: self.mipmaps,
                        ..Default::default()
                    },
                    &self.sprites.pages[page],
                    Some(&self.lightmap),
                )?;
                batch.draw(
                    &mut self.device,
                    &arena.mesh,
                    &mut self.index_scratch,
                    self.solid_reused,
                    dense.then(|| index_budget.saturating_sub(cached_bytes)),
                )?;
                if dense {
                    cached_bytes += batch.cached_bytes();
                }
            }
            self.section_draws = (self.device.draws - before) as usize;
            self.device.profile_pass(dx::SubmitPass::DynamicEntities);
            self.device.transform(
                Mat4::IDENTITY.to_cols_array(),
                view.view.to_cols_array(),
                projection.to_cols_array(),
            )?;
            self.stamp("terrain solid", &mut clock);
            self.draw_entities(frame, frame.entities, fog, 0)?;
            self.draw_entities(frame, frame.world_primitives.quads, fog, 2)?;
            self.device.profile_pass(dx::SubmitPass::StaticEntities);
            if self.static_entities.enabled() {
                self.static_entities
                    .begin(frame, self.device.resets, self.hud.atlas.revision());
            }
            for section in frame.static_block_entities {
                if !frame
                    .visible_static_block_entity_sections
                    .contains(&section.section)
                {
                    continue;
                }
                let origin =
                    (DVec3::from_array(section.origin.map(f64::from)) - frame.camera).as_vec3();
                self.device.transform(
                    Mat4::from_translation(origin).to_cols_array(),
                    view.view.to_cols_array(),
                    projection.to_cols_array(),
                )?;
                self.draw_static_entity_section(frame, section, fog)?;
            }
            self.stamp("entities", &mut clock);
            self.device.profile_pass(dx::SubmitPass::Translucent);
            let before_translucent = self.device.draws;
            let before_translucent_binds = self.device.binds;
            for v in visible.iter().rev() {
                let key = &v.key;
                if self.sections.at(v.slot).translucent.is_empty() {
                    continue;
                }
                self.device.transform(
                    Mat4::from_translation(mesh::region_origin(
                        mesh::region_key(*key),
                        self.layout,
                        frame.camera,
                    ))
                    .to_cols_array(),
                    view.view.to_cols_array(),
                    projection.to_cols_array(),
                )?;
                let section = self.sections.at(v.slot);
                self.translucent_stats[2] += section.translucent.len() as u64;
                let mut order = section.translucent_order.borrow_mut();
                let order = order.update(
                    &section.translucent,
                    frame.camera.to_array(),
                    [key.0 * 16, self.layout.min_y() + key.1 * 16, key.2 * 16],
                );
                let state = DrawState {
                    blend: Blend::Alpha,
                    // Basic transparency: far sections/pages first, no depth
                    // writes between approximate alpha batches. Cull back faces
                    // to avoid double-blending ice without per-quad draw calls.
                    depth_write: false,
                    cull: true,
                    alpha_test: false,
                    fog,
                    fog_program: solid_cache::section_fog(&self.sections, v, fog),
                    linear: self.linear,
                    mipmaps: self.mipmaps,
                    ..Default::default()
                };
                if section.gpu.is_empty() {
                    section.vertices.decode(&mut self.decode_scratch)?;
                }
                for &at in order {
                    let packet = &section.translucent[at];
                    self.device.state(
                        state,
                        &self.sprites.pages[packet.page],
                        Some(&self.lightmap),
                    )?;
                    mesh::draw_range(
                        &mut self.device,
                        &self.arenas,
                        section,
                        packet.first,
                        packet.count,
                        &self.decode_scratch,
                    )?;
                }
            }
            self.visible = visible;
            self.stamp("terrain translucent", &mut clock);
            self.translucent_stats[0] = self.device.draws - before_translucent;
            self.translucent_stats[1] = self.device.binds - before_translucent_binds;
            self.section_draws += (self.device.draws - before_translucent) as usize;
            self.section_draw_calls = self.section_draws;
            self.device.profile_pass(dx::SubmitPass::DynamicEntities);
            self.device.transform(
                Mat4::IDENTITY.to_cols_array(),
                view.view.to_cols_array(),
                projection.to_cols_array(),
            )?;
            self.draw_entities(frame, frame.entities, fog, 1)?;
            self.device.profile_pass(dx::SubmitPass::Effects);
            self.draw_lightning(frame)?;
            self.draw_entities(frame, frame.additive, None, 2)?;
            self.draw_entities(frame, frame.see_through, None, 3)?;
            self.draw_entities(frame, frame.world_primitives.quads_through, None, 3)?;
            self.draw_weather(frame, fog)?;
            self.draw_lines(frame)?;
            self.device.clear_depth()?;
            let hand_projection = View::screen(
                frame.yaw,
                frame.pitch,
                frame.hand_fov_degrees,
                view.aspect,
                frame.depth_far,
            )
            .projection
                * frame.bob;
            self.device.transform(
                Mat4::IDENTITY.to_cols_array(),
                view.view.to_cols_array(),
                hand_projection.to_cols_array(),
            )?;
            self.draw_entities(frame, frame.hand, None, 2)?;
        }
        self.stamp("effects/hand", &mut clock);
        if self.device.finish_world()? {
            self.stamp("world resolve", &mut clock);
        }
        self.device.profile_pass(dx::SubmitPass::Gui);
        self.draw_gui(frame)?;
        self.stamp("GUI draw", &mut clock);
        self.save_screenshot();
        if self.capture.take().is_some() {
            self.capture_result = Some(Err(
                "GPU render captures are unavailable on Direct3D 9; use the chat profiler".into(),
            ));
        }
        Ok(())
    }
    fn entity_batches(
        &mut self,
        frame: &Frame,
        input: &[Vertex],
        fog: Option<dx::Fog>,
        phase: u8,
        compact: bool,
    ) -> Result<EntityBatches> {
        let pixels = self.hud.atlas.entity_or_main(compact);
        let mut batches = Vec::new();
        let mut sprite_batches = Vec::new();
        let mut glints = Vec::new();
        let mut armor_glint = None;
        for quad in input.chunks_exact(4) {
            let overlay = quad[0].layer & 0xffff;
            if overlay == 13 {
                continue;
            }
            // OIT_ENTITY only selects the modern optional OIT path. Classic
            // entityTranslucent (including the player's skin) still writes Z.
            // Translucent items/fluid also precede translucent terrain and write
            // depth; only shadows/particles/energy are late, read-only overlays.
            let transparent = matches!(overlay, 14 | 15 | 34 | 45);
            if (phase == 0 && transparent) || (phase == 1 && !transparent) {
                continue;
            }
            let unlit = matches!(overlay, 2 | 4 | 6 | 9 | 10 | 11 | 12 | 37);
            let additive = matches!(overlay, 2 | 3 | 4 | 37 | 45);
            let state = DrawState {
                blend: if matches!(overlay, 11 | 12) {
                    Blend::Cracks
                } else if additive {
                    Blend::Additive
                } else {
                    Blend::Alpha
                },
                lightmap: !unlit,
                hurt_overlay: matches!(overlay, 1 | 8).then(|| match frame.pvp.hurt_texel {
                    0 => u32::from_le_bytes(crate::overlay::HURT_TEXEL),
                    texel => texel,
                }),
                depth_test: phase != 3,
                depth_write: !transparent && !matches!(overlay, 2 | 3 | 4 | 11 | 12) && phase != 3,
                cull: quad[0].normal[3] == 1 || quad[0].normal[3] == crate::mesher::SHADED_CULL_W,
                alpha_test: true,
                alpha_ref: 25,
                fog,
                linear: self.linear,
                mipmaps: false,
                ..Default::default()
            };
            let mut vertices: [dx::Vertex; 4] = std::array::from_fn(|i| {
                let mut v = vertex(&quad[i]);
                let native_model = matches!(overlay, 42..=44);
                let normal = if native_model {
                    Vec3::new(
                        quad[i].glint_uv[0],
                        quad[i].glint_uv[1],
                        f32::from_le_bytes(quad[i].normal.map(|b| b as u8)),
                    )
                } else {
                    Vec3::new(
                        f32::from(quad[i].normal[0]),
                        f32::from(quad[i].normal[1]),
                        f32::from(quad[i].normal[2]),
                    ) / 127.0
                };
                if (native_model && normal != Vec3::ZERO)
                    || (!native_model && quad[i].normal[3] != 0 && quad[i].normal[3] != 1)
                {
                    let a = Vec3::new(0.2, 1.0, -0.7).normalize();
                    let b = Vec3::new(-0.2, if frame.nether_lighting { -1.0 } else { 1.0 }, 0.7)
                        .normalize();
                    let shade =
                        ((normal.dot(a).max(0.0) + normal.dot(b).max(0.0)) * 0.6 + 0.4).min(1.0);
                    for shift in [0, 8, 16] {
                        let channel = (v.color >> shift & 255) as f32 * shade;
                        v.color = v.color & !(255 << shift) | (channel as u32) << shift;
                    }
                }
                v
            });
            if matches!(overlay, 7 | 8 | 33 | 36) && frame.glint_alpha > 0.0 {
                let armor = overlay == 36;
                let transform = armor.then(|| {
                    *armor_glint.get_or_insert_with(|| dx::ArmorGlint::new(frame.glint_ticks))
                });
                let mut foil = vertices;
                for (v, source) in foil.iter_mut().zip(quad) {
                    v.light = transform.map_or(source.glint_uv, |t| t.uv(source.glint_uv));
                    v.color =
                        dx::glint_color(frame.glint_alpha, frame.pvp.glint, (v.color >> 24) as u8);
                }
                let state = DrawState {
                    glint: if armor { 2 } else { 1 },
                    blend: Blend::Additive,
                    lightmap: false,
                    hurt_overlay: None,
                    depth_write: false,
                    equal_depth: state.depth_test,
                    fog: state.fog.map(|f| dx::Fog {
                        color: [0.0, 0.0, 0.0, f.color[3]],
                        ..f
                    }),
                    ..state
                };
                self.hud
                    .atlas
                    .polygon(&foil, |page, p| batch(&mut glints, page, state, p, false));
            }
            if overlay == 6 && quad[0].normal == crate::mesher::CULL_BACK {
                let (u0, u1) = uv_extent(&vertices, 0);
                let rect = [
                    u0,
                    (quad[0].light & 0xffff) as f32 / 65535.0,
                    u1,
                    (quad[0].light >> 16) as f32 / 65535.0,
                ];
                if let Some((page, limit)) = pixels.repeat_v_texture(&mut self.device, rect)? {
                    beam_bands(&vertices, limit as i32, |poly, band| {
                        for v in poly.iter_mut() {
                            v.uv = [(v.uv[0] - u0) / (u1 - u0), v.uv[1] - band];
                        }
                        batch(
                            &mut batches,
                            page,
                            DrawState {
                                wrap_v: true,
                                ..state
                            },
                            poly,
                            false,
                        );
                        Ok(())
                    })?;
                } else {
                    // A fallback quad is expanded in bounded pieces at submission time.
                    batches.push(Batch {
                        page: 0,
                        state,
                        vertices: vertices.to_vec(),
                        beam_v: Some([rect[1], rect[3]]),
                    });
                }
            } else if matches!(overlay, 41..=44) {
                let layer = if overlay == 41 {
                    quad[0].glint_uv[0] as u32
                } else {
                    quad[0].layer >> 16
                };
                self.sprites.quad(layer, &vertices, |page, p| {
                    batch(
                        &mut sprite_batches,
                        page,
                        DrawState {
                            mipmaps: self.mipmaps,
                            ..state
                        },
                        p,
                        false,
                    )
                })?;
            } else {
                if matches!(overlay, 9 | 10) {
                    let r = frame.end_portal_sprites[1];
                    for (i, v) in vertices.iter_mut().enumerate() {
                        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][i];
                        v.uv = [r[0] + uv[0] * r[2], r[1] + uv[1] * r[3]];
                    }
                }
                if matches!(overlay, 11 | 12) {
                    let rect = frame.crumbling_sprite;
                    repeat_quad(&vertices, rect, |p| {
                        pixels.polygon(p, |page, poly| {
                            batch(&mut batches, page, state, poly, false);
                        });
                    });
                } else {
                    pixels.polygon(&vertices, |page, p| {
                        batch(&mut batches, page, state, p, false);
                    });
                }
            }
        }
        Ok(EntityBatches {
            pixels: batches,
            sprites: sprite_batches,
            glints,
        })
    }
    fn draw_entities(
        &mut self,
        frame: &Frame,
        input: &[Vertex],
        fog: Option<dx::Fog>,
        phase: u8,
    ) -> Result<()> {
        let dynamic = matches!(phase, 0 | 1);
        let mut dynamic_laps = dynamic.then(crate::spans::laps);
        let mut compact = dynamic && self.hud.atlas.entity_view().is_some();
        let mut batches = self.entity_batches(frame, input, fog, phase, compact)?;
        if let Some(laps) = &mut dynamic_laps {
            laps.lap("d3d9_dynamic_entity_convert");
        }
        if compact {
            // Capacity misses already use main UVs. Only an upload failure
            // needs to remap the pass before any of its draws reach the device.
            if let Err(error) = self
                .hud
                .atlas
                .entity_or_main(true)
                .prepare_compact(&mut self.device)
            {
                if dx::is_device_lost(error.as_ref()) {
                    return Err(error);
                }
                log::warn!(
                    "D3D9 dynamic entity page unavailable ({error}); using the shared atlas"
                );
                self.hud.atlas.drop_entity_page();
                compact = false;
                batches = self.entity_batches(frame, input, fog, phase, false)?;
            }
        }
        let EntityBatches {
            pixels: batches,
            sprites: sprite_batches,
            glints,
        } = batches;
        if dynamic && self.entity_staging {
            let page_breaks = if !compact && (self.entity_page_pending || self.diagnostics) {
                batches
                    .windows(2)
                    .filter(|pair| pair[0].state == pair[1].state && pair[0].page != pair[1].page)
                    .count()
            } else {
                0
            };
            if self.diagnostics {
                self.entity_ranges[0] += batches.len() as u64;
                self.entity_ranges[1] += sprite_batches.len() as u64;
                self.entity_ranges[2] += glints.len() as u64;
                self.entity_ranges[3] += page_breaks as u64;
                self.entity_ranges[4] += u64::from(
                    compact && batches.iter().any(|batch| batch.page == atlas::ENTITY_PAGE),
                );
            }
            if self.entity_page_pending && page_breaks >= ENTITY_PAGE_BREAKS {
                self.entity_page_pending = false;
                if self.texture_budget.saturating_sub(self.texture_bytes())
                    >= atlas::ENTITY_PAGE_BYTES
                {
                    match self.hud.atlas.enable_entity_page() {
                        Ok(()) => log::info!(
                            "D3D9 dynamic entity page enabled after {page_breaks} adjacent page breaks"
                        ),
                        Err(error) => {
                            log::warn!("D3D9 dynamic entity page metadata unavailable ({error})")
                        }
                    }
                } else {
                    log::info!("D3D9 dynamic entity page skipped for the current texture budget");
                }
            }
        }
        let staged = self.entity_staging
            && dynamic
            && self.try_stage_entity_batches(&batches, &sprite_batches, compact)?;
        if !staged {
            draw_pixels(
                &mut self.device,
                self.hud.atlas.entity_or_main(compact),
                &self.lightmap,
                &batches,
                false,
            )?;
            for b in sprite_batches {
                self.device
                    .state(b.state, &self.sprites.pages[b.page], Some(&self.lightmap))?;
                self.device.draw_transient(&b.vertices)?;
            }
        }
        #[cfg(feature = "legacy-d3d9-dev")]
        let stage_glint = dynamic
            && self.entity_staging
            && self
                .device
                .benchmark_active(dx::benchmark::Trial::GlintStaging);
        #[cfg(not(feature = "legacy-d3d9-dev"))]
        let stage_glint = false;
        self.draw_glints(&glints, frame.pvp.glint, stage_glint)?;
        if let Some(laps) = &mut dynamic_laps {
            laps.lap("d3d9_dynamic_entity_submit");
        }
        Ok(())
    }
    /// Preserve the existing HUD-page batches, then sprite batches, then foil.
    /// The experiment changes upload granularity, not material/alpha draw order.
    fn try_stage_entity_batches(
        &mut self,
        batches: &[Batch],
        sprites: &[Batch],
        compact: bool,
    ) -> Result<bool> {
        if batches.is_empty() && sprites.is_empty() {
            return Ok(false);
        }
        if batches.iter().any(|b| b.beam_v.is_some()) {
            return Ok(false);
        }
        let pixels = self.hud.atlas.entity_or_main(compact);
        pixels.prepare(&mut self.device)?;
        let textures = pixels.draw_textures();
        let drawable = batches.iter().filter(|b| textures.get(b.page).is_some());
        let count = drawable
            .clone()
            .chain(sprites.iter())
            .try_fold(0usize, |n, b| n.checked_add(b.vertices.len()));
        let Some(count) = count.filter(|&n| n <= self.device.max_vertices()) else {
            self.entity_stage_stats[1] += 1;
            return Ok(false);
        };
        if count == 0 {
            return Ok(false);
        }
        self.entity_stage_vertices.clear();
        for b in drawable.chain(sprites.iter()) {
            self.entity_stage_vertices.extend_from_slice(&b.vertices);
        }
        // One bounded extra CPU consolidation copy; native uploaded bytes and
        // draw NumVertices remain unchanged. Measure this cost in the A/B.
        let stream = match self.device.stage_quads(&self.entity_stage_vertices) {
            Ok(stream) => stream,
            Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
            Err(error) => {
                self.entity_stage_stats[1] += 1;
                if !self.entity_stage_warned {
                    log::warn!("D3D9 entity staging failed ({error}); keeping per-batch uploads");
                    self.entity_stage_warned = true;
                }
                return Ok(false);
            }
        };
        let mut first = 0;
        for b in batches {
            let Some(texture) = textures.get(b.page) else {
                continue;
            };
            self.device.state(b.state, texture, Some(&self.lightmap))?;
            let end = first + b.vertices.len() as u32;
            self.device.draw_staged_quads(&stream, first..end)?;
            first = end;
        }
        for b in sprites {
            self.device
                .state(b.state, &self.sprites.pages[b.page], Some(&self.lightmap))?;
            let end = first + b.vertices.len() as u32;
            self.device.draw_staged_quads(&stream, first..end)?;
            first = end;
        }
        debug_assert_eq!(first as usize, count);
        self.entity_stage_stats[0] += 1;
        Ok(true)
    }
    fn draw_panorama(&mut self, frame: &Frame) -> Result<()> {
        if self.panorama.is_none() {
            let pixels = self.panorama_faces.texels();
            self.panorama = Some(Pixels::new(
                &mut self.device,
                &pixels,
                self.panorama_faces.size,
                self.panorama_faces.size * 6,
                self.texture_budget / 4,
            )?);
        }
        let panorama = self.panorama.as_ref().ok_or("D3D9 panorama missing")?;
        self.device.transform(
            Mat4::IDENTITY.to_cols_array(),
            Mat4::IDENTITY.to_cols_array(),
            panorama::projection(
                self.config.width,
                self.config.height,
                frame.yaw,
                frame.pitch,
            )
            .to_cols_array(),
        )?;
        let mut batches = Vec::new();
        for face in 0..6 {
            let vertices = std::array::from_fn::<_, 4, _>(|i| {
                let [s, t] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]][i];
                let pos = match face {
                    0 => [1.0, -t, -s],
                    1 => [-1.0, -t, s],
                    2 => [s, 1.0, t],
                    3 => [s, -1.0, -t],
                    4 => [s, -t, 1.0],
                    _ => [-s, -t, -1.0],
                };
                dx::Vertex {
                    pos,
                    uv: [(s + 1.0) / 2.0, (face as f32 + (t + 1.0) / 2.0) / 6.0],
                    color: u32::MAX,
                    light: [0.0; 2],
                }
            });
            panorama.polygon(&vertices, |page, p| {
                batch(
                    &mut batches,
                    page,
                    DrawState {
                        lightmap: false,
                        depth_test: false,
                        depth_write: false,
                        cull: false,
                        alpha_test: false,
                        linear: true,
                        mipmaps: false,
                        ..Default::default()
                    },
                    p,
                    false,
                )
            });
        }
        draw_pixels(
            &mut self.device,
            panorama.view(),
            &self.lightmap,
            &batches,
            false,
        )
    }
    fn draw_gui(&mut self, frame: &Frame) -> Result<()> {
        let mut phases = crate::spans::laps();
        let before = [
            self.device.draws,
            self.device.binds,
            self.device.stream_uploads,
            self.device.stream_locks,
        ];
        let identity = Mat4::IDENTITY.to_cols_array();
        self.device.transform(identity, identity, identity)?;
        let mut overlays = Vec::new();
        for q in frame.screen {
            let input = q.vertices.map(|v| dx::Vertex {
                pos: [v.pos[0], v.pos[1], 0.5],
                uv: v.uv,
                color: dx::argb(v.color),
                light: [0.0; 2],
            });
            let r = q.vertices[0].rect;
            let state = DrawState {
                blend: if q.blend == crate::screen_effects::Blend::Vignette {
                    Blend::Multiply
                } else {
                    Blend::Alpha
                },
                lightmap: false,
                cull: false,
                depth_test: false,
                depth_write: false,
                alpha_test: false,
                mipmaps: false,
                ..Default::default()
            };
            repeat_quad(&input, [r[0], r[1], r[2] - r[0], r[3] - r[1]], |p| {
                self.hud.atlas.polygon(p, |page, poly| {
                    batch(&mut overlays, page, state, poly, false)
                })
            });
        }
        draw_pixels(
            &mut self.device,
            self.hud.atlas.view(),
            &self.lightmap,
            &overlays,
            false,
        )?;
        self.draw_crosshair(frame)?;
        let h = frame.hud;
        self.pictures
            .retain(|id, _| h.pictures.iter().any(|p| p.legacy_data().0 == *id));
        for picture in &h.pictures {
            let (id, width, height, pixels, _) = picture.legacy_data();
            if let std::collections::hash_map::Entry::Vacant(entry) = self.pictures.entry(id) {
                entry.insert(Pixels::new(
                    &mut self.device,
                    pixels,
                    width,
                    height,
                    self.texture_budget / 4,
                )?);
            }
        }
        let scale = h.scale;
        // D3D9's pixel centre is an integer; compensate by half a framebuffer pixel.
        let projection = Mat4::from_cols_array(&[
            2.0 * scale / self.config.width as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 * scale / self.config.height as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            -1.0 - 1.0 / self.config.width as f32,
            1.0 + 1.0 / self.config.height as f32,
            0.0,
            1.0,
        ]);
        self.device
            .transform(identity, identity, projection.to_cols_array())?;
        self.gui_vertices.clear();
        self.gui_commands.clear();
        self.gui_cache.begin();
        self.gui_depth.begin();
        for (range, blend) in h.runs() {
            let input = &h.vertices[range.start as usize..range.end as usize];
            let (scissor, depth) = match blend {
                crate::hud::Blend::Depth(r) | crate::hud::Blend::DepthGlint(r) => (Some(r), true),
                _ => (None, false),
            };
            if depth
                && !matches!(blend, crate::hud::Blend::DepthGlint(_))
                && self.gui_depth.clear_for(input.iter().map(|v| v.pos), scale)
            {
                self.gui_commands.push(GuiCommand::ClearDepth);
            }
            let state = DrawState {
                blend: match blend {
                    crate::hud::Blend::Invert => Blend::Invert,
                    crate::hud::Blend::Additive | crate::hud::Blend::DepthGlint(_) => {
                        Blend::Additive
                    }
                    crate::hud::Blend::Vignette => Blend::Multiply,
                    _ => Blend::Alpha,
                },
                lightmap: false,
                cull: false,
                depth_test: depth,
                depth_write: depth && !matches!(blend, crate::hud::Blend::DepthGlint(_)),
                reverse_z: false,
                alpha_test: depth,
                alpha_ref: 25,
                mipmaps: false,
                scissor,
                ..Default::default()
            };
            let picture = if let crate::hud::Blend::Picture(id) = blend {
                Some(id)
            } else {
                None
            };
            let pixels = picture
                .and_then(|id| self.pictures.get(&id))
                .unwrap_or(&self.hud.atlas);
            self.gui_cache.append(
                gui_cache::GuiRun {
                    source: input,
                    glint_color: dx::glint_color(h.glint_alpha, h.glint_colour, 255),
                    pixels,
                    state,
                    picture,
                    cache: matches!(
                        blend,
                        crate::hud::Blend::Alpha | crate::hud::Blend::Depth(_)
                    ),
                },
                &mut self.gui_batches,
                &mut self.gui_vertices,
                &mut self.gui_commands,
            );
        }
        self.gui_cache.finish();
        phases.lap("d3d9_gui_prepare");
        self.gui_screen.clear();
        if self.gui_projected && self.device.caps.software_vertices {
            self.gui_screen.extend(
                self.gui_vertices
                    .iter()
                    .map(|v| dx::ScreenVertex::from_gui(v, scale)),
            );
        }
        phases.lap("d3d9_gui_project");
        self.hud.atlas.prepare(&mut self.device)?;
        for picture in self.pictures.values() {
            picture.prepare(&mut self.device)?;
        }
        phases.lap("d3d9_gui_textures");
        let screen = if !self.gui_screen.is_empty()
            && self.gui_screen.len() <= self.device.max_vertices()
        {
            match self.device.stage_screen(&self.gui_screen) {
                Ok(screen) => Some(screen),
                Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
                Err(error) => {
                    log::warn!(
                        "D3D9 projected GUI stream unavailable ({error}); using the ordinary FFP GUI path"
                    );
                    self.gui_projected = false;
                    self.gui_screen.clear();
                    None
                }
            }
        } else {
            None
        };
        let staged = if self.gui_screen.is_empty()
            && !self.gui_vertices.is_empty()
            && self.gui_vertices.len() <= self.device.max_vertices()
        {
            Some(self.device.stage_triangles(&self.gui_vertices)?)
        } else {
            None
        };
        phases.lap("d3d9_gui_upload");
        for command in &self.gui_commands {
            let GuiCommand::Draw {
                page,
                state,
                picture,
                range,
            } = command
            else {
                self.device.clear_gui_depth()?;
                continue;
            };
            let pixels = picture
                .and_then(|id| self.pictures.get(&id))
                .unwrap_or(&self.hud.atlas);
            let textures = pixels.textures();
            let Some(texture) = &textures[*page] else {
                continue;
            };
            let second = if state.glint != 0 {
                self.glint.texture(state.glint, h.glint_colour)
            } else {
                &self.lightmap
            };
            self.device.state(*state, texture, Some(second))?;
            // The staged TEX2 stream already contains foil UVs. Reuse it for
            // every command: a transient foil upload would invalidate its serial
            // before the next ordinary widget. SWVP's separate XYZRHW stream
            // lacks the second UV, so only that path needs transient foil draws.
            if state.glint != 0 && staged.is_none() {
                self.device
                    .draw_triangles(&self.gui_vertices[range.start as usize..range.end as usize])?;
            } else if let Some(screen) = &screen {
                self.device.draw_screen(screen, range.clone())?;
            } else if let Some(staged) = &staged {
                self.device.draw_staged_triangles(staged, range.clone())?;
            } else if !self.gui_screen.is_empty() {
                self.device.draw_screen_triangles(
                    &self.gui_screen[range.start as usize..range.end as usize],
                )?;
            } else {
                self.device
                    .draw_triangles(&self.gui_vertices[range.start as usize..range.end as usize])?;
            }
        }
        phases.lap("d3d9_gui_submit");
        let after = [
            self.device.draws,
            self.device.binds,
            self.device.stream_uploads,
            self.device.stream_locks,
        ];
        self.gui_stats = std::array::from_fn(|i| after[i] - before[i]);
        Ok(())
    }
    fn draw_crosshair(&mut self, frame: &Frame) -> Result<()> {
        if frame.debug_crosshair.is_empty() {
            return Ok(());
        }
        let view = View::screen(
            frame.yaw,
            frame.pitch,
            frame.hand_fov_degrees,
            self.config.width as f32 / self.config.height.max(1) as f32,
            frame.depth_far,
        );
        let projection = view.projection * view.view;
        let size = glam::Vec2::new(self.config.width as f32, self.config.height as f32);
        let mut vertices = Vec::with_capacity(frame.debug_crosshair.len() * 4);
        // Match the shared line renderer's pixel widths, including the black 4px
        // outline underneath each coloured 2px axis. No hand bob or world fog.
        for line in frame.debug_crosshair {
            let a = projection.project_point3(Vec3::new(line[0], line[1], line[2]));
            let b = projection.project_point3(Vec3::new(line[3], line[4], line[5]));
            if !a.is_finite() || !b.is_finite() {
                continue;
            }
            let delta = (b - a).truncate() * size;
            let offset = glam::Vec2::new(-delta.y, delta.x).normalize_or_zero() * line[7] / size;
            for (p, sign) in [(a, -1.0), (b, -1.0), (b, 1.0), (a, 1.0)] {
                vertices.push(dx::Vertex {
                    pos: [p.x + sign * offset.x, p.y + sign * offset.y, p.z],
                    color: line[6].to_bits(),
                    uv: [0.5; 2],
                    light: [0.0; 2],
                });
            }
        }
        self.device.state(
            DrawState {
                blend: Blend::Alpha,
                cull: false,
                depth_test: false,
                depth_write: false,
                alpha_test: false,
                lightmap: false,
                mipmaps: false,
                ..Default::default()
            },
            &self.white,
            None,
        )?;
        self.device.draw_transient(&vertices)
    }
    fn save_screenshot(&mut self) {
        let Some(request) = self.screenshot.take() else {
            return;
        };
        self.screenshot_result = Some(
            self.device
                .screenshot()
                .map(|(width, height, pixels)| {
                    let (pixels, size) = if let Some(size) = request.size {
                        (gfx::square_downscale(&pixels, width, height, size), size)
                    } else {
                        (pixels, [width, height])
                    };
                    gfx::PendingScreenshot::sdr(request.path, request.background, size, pixels)
                })
                .map_err(|error| error.to_string()),
        );
    }
}
fn uv_extent(quad: &[dx::Vertex; 4], axis: usize) -> (f32, f32) {
    quad.iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v.uv[axis]), hi.max(v.uv[axis]))
        })
}

fn beam_bands(
    vertices: &[dx::Vertex; 4],
    step: i32,
    mut emit: impl FnMut(&mut [dx::Vertex], f32) -> Result<()>,
) -> Result<()> {
    let (u0, u1) = uv_extent(vertices, 0);
    let (v_min, v_max) = uv_extent(vertices, 1);
    let mut band = v_min.floor() as i32;
    let last = (v_max.ceil() as i32).max(band.saturating_add(1));
    if band.saturating_add(step) >= last {
        let mut quad = *vertices;
        return emit(&mut quad, band as f32);
    }
    while band < last {
        let end = band.saturating_add(step).min(last);
        let mut poly = atlas::clip_rect(vertices.to_vec(), [u0, band as f32, u1, end as f32]);
        if poly.len() >= 3 {
            emit(&mut poly, band as f32)?;
        }
        band = end;
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn beacon_bands_preserve_winding_and_repeat_across_negative_uvs() {
    let input = [[1.0, 2.25], [1.0, -0.75], [0.0, -0.75], [0.0, 2.25]].map(|uv| dx::Vertex {
        pos: [uv[0], uv[1], 0.0],
        uv,
        color: 0x20ff_8844,
        light: [0.0; 2],
    });
    let mut intervals = Vec::new();
    beam_bands(&input, 1, |poly, band| {
        assert_eq!(poly.len(), 4);
        for v in poly.iter() {
            assert_eq!(v.color, input[0].color);
            assert!((0.0..=1.0).contains(&(v.uv[1] - band)));
            assert_eq!(v.pos[1], v.uv[1]);
        }
        let a = Vec3::from_array(poly[1].pos) - Vec3::from_array(poly[0].pos);
        let b = Vec3::from_array(poly[2].pos) - Vec3::from_array(poly[0].pos);
        assert!(a.cross(b).z < 0.0);
        intervals.push(band);
        Ok(())
    })
    .unwrap();
    assert_eq!(intervals, [-1.0, 0.0, 1.0, 2.0]);
}

fn repeat_quad(input: &[dx::Vertex; 4], rect: [f32; 4], mut emit: impl FnMut(&[dx::Vertex])) {
    let min = std::array::from_fn::<_, 2, _>(|k| {
        input
            .iter()
            .map(|v| v.uv[k])
            .fold(f32::INFINITY, f32::min)
            .floor() as i32
    });
    let max = std::array::from_fn::<_, 2, _>(|k| {
        (input
            .iter()
            .map(|v| v.uv[k])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil() as i32)
            .max(min[k] + 1)
    });
    // Screen effects/weather repeat locally, never across neighbouring atlas sprites.
    for y in min[1]..max[1].min(min[1] + 64) {
        for x in min[0]..max[0].min(min[0] + 64) {
            let mut poly = atlas::clip_rect(
                input.to_vec(),
                [x as f32, y as f32, (x + 1) as f32, (y + 1) as f32],
            );
            for v in &mut poly {
                v.uv = [
                    rect[0] + (v.uv[0] - x as f32) * rect[2],
                    rect[1] + (v.uv[1] - y as f32) * rect[3],
                ];
            }
            if poly.len() >= 3 {
                emit(&poly);
            }
        }
    }
}

impl Renderer {
    fn update_lightmap(&mut self, frame: &Frame) -> Result<()> {
        let sky = &frame.sky;
        let darkness = frame.darkness.1;
        let gamma = (self.gamma - frame.darkness.0).max(0.0);
        let night_vision = frame.night_vision;
        let flicker = frame.block_flicker;
        let boss_darken = frame.boss_darken;
        // lightmap.fsh: per (block, sky) level.
        let brightness = |level: f32| level / (4.0 - 3.0 * level);
        let mut pixels = [0u8; 16 * 16 * 4];
        for s in 0..16 {
            for b in 0..16 {
                let block_level = b as f32 / 15.0;
                let sky_level = s as f32 / 15.0;
                let block_brightness = brightness(block_level) * (1.4 + flicker);
                let sky_brightness = brightness(sky_level) * sky.sky_light_factor;
                // NightVisionColor defaults to 0x999999.
                let mut color =
                    Vec3::from(sky.ambient_light_color).max(Vec3::splat(0.6 * night_vision));
                color += Vec3::from(sky.sky_light_color) * sky_brightness;
                let parabolic = (2.0 * block_level - 1.0).powi(2);
                let block_color = Vec3::from(sky.block_light_tint).lerp(Vec3::ONE, 0.9 * parabolic);
                color += block_color * block_brightness;
                // lightmap.fsh: a boss bar mixes the light toward 70/60/60%.
                color = color.lerp(color * Vec3::new(0.7, 0.6, 0.6), boss_darken);
                color -= Vec3::splat(darkness);
                color = color.clamp(Vec3::ZERO, Vec3::ONE);
                let max = color.max_element().max(1e-6);
                let inverted = 1.0 - max;
                let scaled = 1.0 - inverted.powi(4);
                let not_gamma = color * (scaled / max);
                color = color.lerp(not_gamma, gamma);
                let i = (s * 16 + b) * 4;
                pixels[i] = (color.x * 255.0) as u8;
                pixels[i + 1] = (color.y * 255.0) as u8;
                pixels[i + 2] = (color.z * 255.0) as u8;
                pixels[i + 3] = 255;
            }
        }
        if self.fullbright {
            pixels.fill(255);
        }

        if pixels != self.lightmap_pixels {
            self.device
                .patch(&self.lightmap, 0, 0, 0, 16, 16, &pixels)?;
            self.lightmap_pixels = pixels;
        }
        Ok(())
    }
}
impl Renderer {
    fn draw_weather(&mut self, frame: &Frame, fog: Option<dx::Fog>) -> Result<()> {
        let mut batches = Vec::new();
        let time = frame.weather_time.values;
        for quad in frame.weather.chunks_exact(4) {
            let input: [dx::Vertex; 4] = std::array::from_fn(|i| {
                let v = &quad[i];
                let a = v.animation;
                let phase = if v.snow == 0 {
                    [0.0, (-((time[0] + a[1]) + time[2]) / 32.0 * a[0]) % 32.0]
                } else {
                    let u = f64::from_bits(u64::from(v.bases[0]) | u64::from(v.bases[1]) << 32);
                    let w = f64::from_bits(u64::from(v.bases[2]) | u64::from(v.bases[3]) << 32);
                    let t = time[0] + time[2];
                    [
                        (u + f64::from(t * 0.01 * a[2])) as f32,
                        -(time[1] + time[2]) / 512.0 + (w + f64::from(t * a[3] * 0.001)) as f32,
                    ]
                };
                dx::Vertex {
                    pos: v.pos,
                    uv: [v.uv[0] + phase[0], v.uv[1] + phase[1]],
                    color: dx::rgba_to_argb(v.color),
                    light: light_uv(v.light),
                }
            });
            let state = DrawState {
                blend: Blend::Alpha,
                depth_write: false,
                cull: false,
                alpha_test: false,
                fog,
                mipmaps: false,
                ..Default::default()
            };
            repeat_quad(&input, quad[0].rect, |p| {
                self.hud.atlas.polygon(p, |page, poly| {
                    batch(&mut batches, page, state, poly, false)
                })
            });
        }
        draw_pixels(
            &mut self.device,
            self.hud.atlas.view(),
            &self.lightmap,
            &batches,
            false,
        )
    }
    fn draw_lines(&mut self, frame: &Frame) -> Result<()> {
        let mut vertices = Vec::new();
        for edge in &frame.outline {
            let color = dx::argb([0.0, 0.0, 0.0, 0.7]);
            for i in [0, 3] {
                let position = DVec3::new(edge[i], edge[i + 1], edge[i + 2]) - frame.camera;
                vertices.push(dx::Vertex {
                    pos: position.as_vec3().to_array(),
                    color,
                    uv: [0.5; 2],
                    light: [0.0; 2],
                });
            }
        }
        for line in frame.debug_lines.iter().chain(frame.world_primitives.lines) {
            for i in [0, 3] {
                vertices.push(dx::Vertex {
                    pos: [line[i], line[i + 1], line[i + 2]],
                    color: line[6].to_bits(),
                    uv: [0.5; 2],
                    light: [0.0; 2],
                });
            }
        }
        let state = DrawState {
            blend: Blend::Alpha,
            cull: false,
            depth_write: false,
            lightmap: false,
            mipmaps: false,
            ..Default::default()
        };
        self.device.state(state, &self.white, None)?;
        self.device.draw_lines(&vertices)?;
        vertices.clear();
        for line in frame
            .debug_lines_on_top
            .iter()
            .chain(frame.world_primitives.lines_through)
        {
            for i in [0, 3] {
                vertices.push(dx::Vertex {
                    pos: [line[i], line[i + 1], line[i + 2]],
                    color: line[6].to_bits(),
                    uv: [0.5; 2],
                    light: [0.0; 2],
                });
            }
        }
        self.device.state(
            DrawState {
                depth_test: false,
                ..state
            },
            &self.white,
            None,
        )?;
        self.device.draw_lines(&vertices)?;
        if let Some(border) = &frame.border {
            let mut vertices = Vec::new();
            for &side in &border.sides {
                for v in &border.vertices[side * 4..side * 4 + 4] {
                    vertices.push(dx::Vertex {
                        pos: std::array::from_fn(|i| v[i] + border.transforms.model_offset[i]),
                        uv: [
                            v[3] + border.transforms.texture_offset,
                            v[4] + border.transforms.texture_offset,
                        ],
                        color: dx::argb(border.transforms.color),
                        light: [0.0; 2],
                    });
                }
            }
            self.device.state(
                DrawState {
                    wrap: true,
                    blend: Blend::Additive,
                    ..state
                },
                &self.forcefield,
                None,
            )?;
            self.device.draw_transient(&vertices)?;
        }
        Ok(())
    }
}

impl Renderer {
    fn draw_sky(&mut self, frame: &Frame) -> Result<()> {
        if !frame.skybox || frame.end_sky || frame.sky_blocked {
            return Ok(());
        }
        let fog = frame.fog.map_or(
            [
                frame.sky.fog_color[0],
                frame.sky.fog_color[1],
                frame.sky.fog_color[2],
                1.0,
            ],
            |f| f.color,
        );
        let end = frame
            .fog
            .map_or(frame.distance.min(512.0), |f| f.sky_end)
            .max(0.001);
        let rim = (512.0f32 * 512.0 + 16.0 * 16.0).sqrt();
        let stop = ((end - 16.0) / (rim - 16.0)).clamp(0.0, 1.0);
        let color = |distance: f32| {
            let f = (distance / end).clamp(0.0, 1.0) * fog[3];
            let rgb = Vec3::from_array(frame.sky.sky_color)
                .lerp(Vec3::from_array([fog[0], fog[1], fog[2]]), f);
            dx::argb([rgb.x, rgb.y, rgb.z, 1.0])
        };
        let vertex = |s: f32, sector: usize| {
            let angle = sector as f32 * std::f32::consts::FRAC_PI_4;
            let (sin, cos) = angle.sin_cos();
            dx::Vertex {
                pos: [cos * 512.0 * s, 16.0, sin * 512.0 * s],
                color: color(16.0 + s * (rim - 16.0)),
                uv: [0.0; 2],
                light: [0.0; 2],
            }
        };
        // The same octagonal sky disc and piecewise fog ramp as terrain.wgsl.
        // A ring at saturation keeps interpolated vertex colour from spreading
        // distant fog across the entire sky. Only 24 triangles, no sky shader.
        let mut vertices = Vec::with_capacity(72);
        for i in 0..8 {
            let center = vertex(0.0, 0);
            let a = vertex(stop, i);
            let b = vertex(stop, i + 1);
            let c = vertex(1.0, i);
            let d = vertex(1.0, i + 1);
            vertices.extend_from_slice(&[center, a, b, a, c, d, a, d, b]);
        }
        let sky = View::screen(
            frame.yaw,
            frame.pitch,
            frame.fov_degrees,
            self.config.width as f32 / self.config.height.max(1) as f32,
            1.0e6,
        );
        self.device.transform(
            Mat4::IDENTITY.to_cols_array(),
            sky.view.to_cols_array(),
            sky.projection.to_cols_array(),
        )?;
        self.device.state(
            DrawState {
                depth_test: false,
                depth_write: false,
                alpha_test: false,
                cull: false,
                lightmap: false,
                mipmaps: false,
                ..Default::default()
            },
            &self.white,
            None,
        )?;
        self.device.draw_triangles(&vertices)
    }
    fn draw_end_sky(&mut self, frame: &Frame) -> Result<()> {
        let Some(rect) = frame
            .end_sky_sprite
            .filter(|_| frame.end_sky && !frame.sky_blocked)
        else {
            return Ok(());
        };
        let mut batches = Vec::new();
        for face in 0..6 {
            let input = std::array::from_fn::<_, 4, _>(|i| {
                let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][i];
                let [s, t] = uv.map(|v| v * 2.0 - 1.0);
                let pos = match face {
                    0 => [1.0, -t, -s],
                    1 => [-1.0, -t, s],
                    2 => [s, 1.0, t],
                    3 => [s, -1.0, -t],
                    4 => [s, -t, 1.0],
                    _ => [-s, -t, -1.0],
                };
                dx::Vertex {
                    pos,
                    uv: [
                        rect[0] + uv[0] * (rect[2] - rect[0]),
                        rect[1] + uv[1] * (rect[3] - rect[1]),
                    ],
                    color: 0xff282828,
                    light: [0.0; 2],
                }
            });
            self.hud.atlas.polygon(&input, |page, p| {
                batch(
                    &mut batches,
                    page,
                    DrawState {
                        lightmap: false,
                        cull: false,
                        depth_test: false,
                        depth_write: false,
                        alpha_test: false,
                        mipmaps: false,
                        ..Default::default()
                    },
                    p,
                    false,
                )
            });
        }
        draw_pixels(
            &mut self.device,
            self.hud.atlas.view(),
            &self.lightmap,
            &batches,
            false,
        )
    }
}

impl Renderer {
    fn draw_lightning(&mut self, frame: &Frame) -> Result<()> {
        self.lightning_cache
            .retain(|seed, _| frame.lightning.bolts.iter().any(|b| b.seed == *seed));
        for bolt in frame.lightning.bolts {
            let cached = self.lightning_cache.entry(bolt.seed).or_insert_with(|| {
                (
                    gfx::lightning::legacy_geometry(bolt.seed, frame.lightning.uv),
                    frame.lightning.uv,
                )
            });
            if cached.1 != frame.lightning.uv {
                *cached = (
                    gfx::lightning::legacy_geometry(bolt.seed, frame.lightning.uv),
                    frame.lightning.uv,
                );
            }
            let origin = (DVec3::from_array(bolt.at) - frame.camera).as_vec3();
            let mut vertices = cached.0.clone();
            for vertex in &mut vertices {
                vertex.pos = (Vec3::from_array(vertex.pos) + origin).to_array();
            }
            self.draw_entities(frame, &vertices, None, 2)?;
        }
        Ok(())
    }
}
