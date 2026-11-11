use super::*;
use dx::{Blend, DrawState};

struct Batch {
    page: usize,
    state: DrawState,
    vertices: Vec<dx::Vertex>,
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
        .is_none_or(|b| b.page != page || b.state != state)
    {
        out.push(Batch {
            page,
            state,
            vertices: Vec::new(),
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
    pixels: &Pixels,
    lightmap: &dx::Texture,
    batches: &[Batch],
    triangles: bool,
) -> Result<()> {
    for b in batches {
        let Some(texture) = &pixels.pages[b.page] else {
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
    pub fn camera_frustum(
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
    pub fn camera_frustum_with_jitter(
        &self,
        fov_degrees: f32,
        yaw: f32,
        pitch: f32,
        bob: Mat4,
        depth_far: f32,
    ) -> Frustum {
        self.camera_frustum(fov_degrees, yaw, pitch, bob, depth_far)
    }
    pub fn render(&mut self, frame: &Frame) -> Result<bool> {
        if self.suspended {
            return Ok(false);
        }
        if let Some(error) = self.pending_error.take() {
            return Err(error.into());
        }
        let started = Instant::now();
        let clear = if frame.end_sky {
            [0.0; 3]
        } else if frame.skybox && !frame.sky_blocked {
            frame.sky.sky_color
        } else {
            frame.fog.map_or(frame.sky.fog_color, |f| {
                [f.color[0], f.color[1], f.color[2]]
            })
        };
        if !self.device.begin(clear)? {
            self.last_wait = started.elapsed();
            self.last_encode = Duration::ZERO;
            return Ok(false);
        }
        let result = self.draw_frame(frame);
        if let Err(e) = result {
            self.device.cancel_frame();
            return Err(e);
        }
        self.last_encode = started.elapsed();
        let wait = Instant::now();
        let shown = self.device.finish()?;
        self.last_wait = wait.elapsed();
        self.uploaded_bytes = std::mem::take(&mut self.device.uploads);
        self.frames += 1;
        self.stage_times = vec![
            (
                "D3D9 CPU frame".into(),
                self.last_encode.as_secs_f32() * 1000.0,
            ),
            ("D3D9 Present".into(), self.last_wait.as_secs_f32() * 1000.0),
        ];
        if self.last_log.elapsed() >= Duration::from_secs(5) {
            log::info!(
                "D3D9 frame={} cpu_ms={:.3} present_ms={:.3} draws={} texture_binds={} uploads={} sections={}/{} mesh_bytes={} texture_bytes={}",
                self.frames,
                self.last_encode.as_secs_f64() * 1000.0,
                self.last_wait.as_secs_f64() * 1000.0,
                self.device.draws,
                self.device.binds,
                self.uploaded_bytes,
                self.drawn_sections,
                self.sections.len(),
                self.mesh_bytes,
                self.texture_bytes()
            );
            self.last_log = Instant::now();
        }
        Ok(shown)
    }
    fn texture_bytes(&self) -> u64 {
        self.sprites.layout.bytes()
            + self.hud.atlas.bytes()
            + self.panorama.as_ref().map_or(0, Pixels::bytes)
            + self.pictures.values().map(Pixels::bytes).sum::<u64>()
            + self.forcefield.bytes
            + self.white.bytes
            + self.lightmap.bytes
    }
    fn draw_frame(&mut self, frame: &Frame) -> Result<()> {
        self.update_lightmap(frame)?;
        if self.animated_textures {
            self.sprites.animate(&mut self.device, frame.ticks)?;
        }
        for patch in &frame.hud.patches {
            self.hud.atlas.patch(
                &mut self.device,
                patch.x,
                patch.y,
                patch.width,
                patch.height,
                &patch.rgba,
            )?;
        }
        for (rect, animation, old) in &mut self.hud.sprite_animations {
            let selected = animation.frame(frame.ticks);
            if old.0 == selected {
                continue;
            }
            let (x, y) = rect.texels((self.hud.atlas.width, self.hud.atlas.height));
            let pixels = animation.resized_pixels(selected, rect.w, rect.h);
            self.hud
                .atlas
                .patch(&mut self.device, x, y, rect.w, rect.h, &pixels)?;
            *old = (selected, 0, 0);
        }
        if self.texture_bytes() > self.texture_budget {
            return Err("D3D9 GUI patches exceeded HORIZON_D3D9_TEXTURE_MIB; no graphics settings were changed".into());
        }
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
        if frame.title {
            self.draw_panorama(view)?;
        } else {
            self.panorama = None;
            self.draw_end_sky(frame)?;
            self.draw_entities(frame, frame.celestial, None, 2)?;
            let fog = frame
                .fog
                .map(|f| {
                    (
                        [f.color[0], f.color[1], f.color[2]],
                        f.environmental_start.min(f.render_distance_start),
                        f.environmental_end.min(f.render_distance_end),
                    )
                })
                .or_else(|| {
                    frame
                        .environment_fog
                        .map(|(start, end)| (frame.sky.fog_color, start, end))
                })
                .or(Some((
                    frame.sky.fog_color,
                    frame.distance * 0.75,
                    frame.distance,
                )));
            let mut visible: Vec<(SectionKey, Vec3, f32)> = self
                .sections
                .iter()
                .filter_map(|(&key, section)| {
                    let origin = (DVec3::new(
                        f64::from(key.0) * 16.0,
                        f64::from(self.layout.min_y()) + f64::from(key.1) * 16.0,
                        f64::from(key.2) * 16.0,
                    ) - frame.camera)
                        .as_vec3();
                    let bounds = section.bounds.unwrap_or([Vec3::ZERO, Vec3::splat(16.0)]);
                    let min = origin + bounds[0];
                    let max = origin + bounds[1];
                    let delta = Vec3::ZERO.max(min).min(max);
                    (frustum.intersects(min, max)
                        && delta.x.abs().max(delta.z.abs()) <= frame.distance)
                        .then_some((key, origin, (origin + Vec3::splat(8.0)).length_squared()))
                })
                .collect();
            visible.sort_unstable_by(|a, b| a.2.total_cmp(&b.2));
            self.drawn_sections = visible.len();
            for (key, origin, _) in &visible {
                let section = &self.sections[key];
                self.device.transform(
                    Mat4::from_translation(*origin).to_cols_array(),
                    view.view.to_cols_array(),
                    projection.to_cols_array(),
                )?;
                let mut runs = Vec::new();
                if self.sides {
                    section
                        .sides
                        .visible((-*origin).to_array(), |first, count| {
                            runs.push(first..first + count)
                        });
                } else {
                    runs.push(0..u32::MAX);
                }
                for packet in &section.solid {
                    if !runs
                        .iter()
                        .any(|r| r.start < packet.source.end && packet.source.start < r.end)
                    {
                        continue;
                    }
                    self.device.state(
                        DrawState {
                            alpha_test: !packet.opaque,
                            fog,
                            linear: self.linear,
                            mipmaps: self.mipmaps,
                            ..Default::default()
                        },
                        &self.sprites.pages[packet.page],
                        Some(&self.lightmap),
                    )?;
                    self.device.draw(&packet.mesh, 0, packet.mesh.vertices)?;
                    self.section_draws += 1;
                }
            }
            self.device.transform(
                Mat4::IDENTITY.to_cols_array(),
                view.view.to_cols_array(),
                projection.to_cols_array(),
            )?;
            self.draw_entities(frame, frame.entities, fog, 0)?;
            self.draw_entities(frame, frame.world_primitives.quads, fog, 2)?;
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
                self.draw_entities(frame, &section.vertices, fog, 2)?;
            }
            for (key, origin, _) in visible.iter().rev() {
                self.device.transform(
                    Mat4::from_translation(*origin).to_cols_array(),
                    view.view.to_cols_array(),
                    projection.to_cols_array(),
                )?;
                for packet in &self.sections[key].translucent {
                    self.device.state(
                        DrawState {
                            blend: Blend::Alpha,
                            depth_write: false,
                            cull: false,
                            alpha_test: false,
                            fog,
                            linear: self.linear,
                            mipmaps: self.mipmaps,
                            ..Default::default()
                        },
                        &self.sprites.pages[packet.page],
                        Some(&self.lightmap),
                    )?;
                    self.device.draw(&packet.mesh, 0, packet.mesh.vertices)?;
                    self.section_draws += 1;
                }
            }
            self.section_draw_calls = self.section_draws;
            self.device.transform(
                Mat4::IDENTITY.to_cols_array(),
                view.view.to_cols_array(),
                projection.to_cols_array(),
            )?;
            self.draw_entities(frame, frame.entities, fog, 1)?;
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
        self.draw_gui(frame)?;
        self.save_screenshot();
        if self.capture.take().is_some() {
            self.capture_result = Some(Err(
                "Render captures require the modern client; Direct3D9 logs counters directly"
                    .into(),
            ));
        }
        Ok(())
    }
    fn draw_entities(
        &mut self,
        frame: &Frame,
        input: &[Vertex],
        fog: Option<([f32; 3], f32, f32)>,
        phase: u8,
    ) -> Result<()> {
        let mut batches = Vec::new();
        let mut sprite_batches = Vec::new();
        for quad in input.chunks_exact(4) {
            let overlay = quad[0].layer & 0xffff;
            if overlay == 13 {
                continue;
            }
            let transparent = if matches!(overlay, 41..=44) {
                overlay == 41
            } else {
                matches!(overlay, 14 | 15 | 32 | 33 | 34 | 45)
                    || quad[0].layer & gfx::OIT_ENTITY != 0
            };
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
                depth_test: phase != 3,
                depth_write: !transparent && !additive && phase != 3,
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
            if matches!(overlay, 41..=44) {
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
                        self.hud.atlas.polygon(p, |page, poly| {
                            batch(&mut batches, page, state, poly, false)
                        })
                    });
                } else {
                    self.hud.atlas.polygon(&vertices, |page, p| {
                        batch(&mut batches, page, state, p, false)
                    });
                }
            }
        }
        draw_pixels(
            &mut self.device,
            &self.hud.atlas,
            &self.lightmap,
            &batches,
            false,
        )?;
        for b in sprite_batches {
            self.device
                .state(b.state, &self.sprites.pages[b.page], Some(&self.lightmap))?;
            self.device.draw_transient(&b.vertices)?;
        }
        Ok(())
    }
    fn draw_panorama(&mut self, view: &View) -> Result<()> {
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
            view.view.to_cols_array(),
            view.projection.to_cols_array(),
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
        draw_pixels(&mut self.device, panorama, &self.lightmap, &batches, false)
    }
    fn draw_gui(&mut self, frame: &Frame) -> Result<()> {
        self.device.clear_depth()?;
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
            &self.hud.atlas,
            &self.lightmap,
            &overlays,
            false,
        )?;
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
        for (range, blend) in h.runs() {
            let (scissor, depth) = match blend {
                crate::hud::Blend::Depth(r) | crate::hud::Blend::DepthGlint(r) => (Some(r), true),
                _ => (None, false),
            };
            if depth && !matches!(blend, crate::hud::Blend::DepthGlint(_)) {
                self.device.clear_depth()?;
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
                alpha_test: depth,
                alpha_ref: 25,
                mipmaps: false,
                scissor,
                ..Default::default()
            };
            let mut batches = Vec::new();
            let input = &h.vertices[range.start as usize..range.end as usize];
            let pixels = if let crate::hud::Blend::Picture(id) = blend {
                self.pictures.get(&id).unwrap_or(&self.hud.atlas)
            } else {
                &self.hud.atlas
            };
            for tri in input.chunks_exact(3) {
                // Foil overlays are optional; their base item's geometry was already emitted.
                if tri[0].uv[0] >= 2.0 {
                    continue;
                }
                let vertices = tri
                    .iter()
                    .map(|v| dx::Vertex {
                        pos: [v.pos[0], v.pos[1], v.depth],
                        uv: v.uv,
                        color: dx::rgba_to_argb(v.color),
                        light: [0.0; 2],
                    })
                    .collect::<Vec<_>>();
                pixels.polygon(&vertices, |page, p| {
                    batch(&mut batches, page, state, p, true)
                });
            }
            draw_pixels(&mut self.device, pixels, &self.lightmap, &batches, true)?;
        }
        Ok(())
    }
    fn save_screenshot(&mut self) {
        let Some(request) = self.screenshot.take() else {
            return;
        };
        let result = self
            .device
            .screenshot()
            .and_then(|(width, height, pixels)| {
                let (pixels, size) = if let Some(size) = request.size {
                    (gfx::square_downscale(&pixels, width, height, size), size)
                } else {
                    (pixels, [width, height])
                };
                if let Some(parent) = request.path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut encoder = png::Encoder::new(
                    std::io::BufWriter::new(std::fs::File::create(&request.path)?),
                    size[0],
                    size[1],
                );
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(&pixels)?;
                Ok(size)
            })
            .map_err(|e| e.to_string());
        if request.background {
            self.saved_screenshots.push(SavedScreenshot {
                path: request.path,
                result,
                hdr: None,
            });
        } else {
            self.screenshot_result = Some(result);
        }
    }
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
    fn draw_weather(&mut self, frame: &Frame, fog: Option<([f32; 3], f32, f32)>) -> Result<()> {
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
            &self.hud.atlas,
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
            &self.hud.atlas,
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
