use super::*;
pub(super) struct Packet {
    pub page: usize,
    pub opaque: bool,
    pub first: u32,
    pub count: u32,
    pub source: Range<u32>,
}
pub(super) struct Section {
    pub solid: Vec<Packet>,
    pub translucent: Vec<Packet>,
    pub vertices: Vec<dx::Vertex>,
    pub gpu: Vec<dx::Mesh>,
    pub last_used: u64,
    pub retry_after: u64,
    pub sides: crate::mesher::Sides,
    pub bounds: Option<[Vec3; 2]>,
    pub blocks: Vec<([i32; 3], u32)>,
    pub revision: u64,
    pub bytes: u64,
}
impl Renderer {
    pub fn upload(&mut self, key: SectionKey, mesh: &SectionMesh) {
        let started = Instant::now();
        if let Err(error) = self.upload_section(key, mesh) {
            log::error!("D3D9 section {key:?}: {error}");
            self.pending_error = Some(format!("D3D9 section {key:?}: {error}"));
        }
        self.upload_cpu += started.elapsed();
    }
    fn upload_section(&mut self, key: SectionKey, mesh: &SectionMesh) -> Result<()> {
        let mut groups: BTreeMap<(usize, usize, bool), (Range<u32>, Vec<dx::Vertex>)> =
            BTreeMap::new();
        let sides = if mesh.sides.quads() == mesh.solid.len() as u32 / 4 {
            mesh.sides
        } else {
            crate::mesher::Sides::unsorted(mesh.solid.len() as u32 / 4)
        };
        let thresholds = sides.thresholds().map(|v| v.0);
        for (quad, input) in mesh.solid.chunks_exact(4).enumerate() {
            let bucket = thresholds.map_or(0, |ends| {
                ends.iter()
                    .position(|&end| (quad as u32) < end)
                    .unwrap_or(6)
            });
            let range = thresholds.map_or(0..mesh.solid.len() as u32 / 4, |ends| {
                if bucket == 0 {
                    0..ends[0]
                } else {
                    ends[bucket - 1]..ends[bucket]
                }
            });
            self.sprites.quad(
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &terrain_quad(input),
                |page, poly| {
                    let group = groups
                        .entry((
                            bucket,
                            page,
                            input[0].layer & crate::mesher::OPAQUE_LAYER != 0,
                        ))
                        .or_insert_with(|| (range.clone(), Vec::new()));
                    atlas::quads(poly, &mut group.1);
                },
            )?;
        }
        let mut vertices = Vec::with_capacity(mesh.solid.len() + mesh.translucent.len());
        let mut solid = Vec::new();
        for ((_, page, opaque), (source, part)) in groups {
            solid.push(Packet {
                page,
                opaque,
                first: vertices.len() as u32,
                count: part.len() as u32,
                source,
            });
            vertices.extend(part);
        }
        let mut translucent = Vec::<Packet>::new();
        for input in mesh.translucent.chunks_exact(4) {
            self.sprites.quad(
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &terrain_quad(input),
                |page, poly| {
                    if translucent.last().is_none_or(|p| p.page != page) {
                        translucent.push(Packet {
                            page,
                            opaque: false,
                            first: vertices.len() as u32,
                            count: 0,
                            source: 0..u32::MAX,
                        });
                    }
                    let before = vertices.len();
                    atlas::quads(poly, &mut vertices);
                    translucent.last_mut().unwrap().count += (vertices.len() - before) as u32;
                },
            )?;
        }
        vertices.shrink_to_fit();
        let bytes = vertices.len() as u64 * size_of::<dx::Vertex>() as u64;
        if let Some(old) = self.sections.remove(&key) {
            self.mesh_bytes -= old.bytes;
            self.gpu_mesh_bytes = self.gpu_mesh_bytes.saturating_sub(
                old.gpu
                    .iter()
                    .filter(|b| b.resident())
                    .map(dx::Mesh::bytes)
                    .sum::<u64>(),
            );
        }
        self.revision = self.revision.wrapping_add(1);
        self.sections.insert(
            key,
            Section {
                solid,
                translucent,
                vertices,
                gpu: Vec::new(),
                last_used: 0,
                retry_after: 0,
                sides,
                bounds: mesh.bounds,
                blocks: mesh.block_entities.clone(),
                revision: self.revision,
                bytes,
            },
        );
        self.mesh_bytes += bytes;
        Ok(())
    }
    /// Evict offscreen GPU copies first. CPU geometry remains available for the
    /// bounded transient draw path, so cache pressure never removes blocks or
    /// changes the user's distance. No mesh conversion is done in the frame loop.
    pub(super) fn prepare_sections(&mut self, visible: &[(SectionKey, Vec3, f32)]) -> Result<()> {
        self.gpu_mesh_bytes = 0;
        for section in self.sections.values_mut() {
            if section.gpu.iter().any(|b| !b.resident()) {
                section.gpu.clear();
            }
            self.gpu_mesh_bytes += section.gpu.iter().map(dx::Mesh::bytes).sum::<u64>();
        }
        for (key, _, _) in visible {
            self.sections.get_mut(key).unwrap().last_used = self.frames + 1;
        }
        let needed: u64 = visible
            .iter()
            .map(|(key, _, _)| self.sections[key].bytes)
            .sum();
        if needed > self.mesh_budget && !self.cache_warning {
            log::warn!(
                "D3D9 visible geometry {} MiB exceeds the {} MiB GPU cache target; uncached geometry uses bounded transient draws, without changing render distance",
                needed >> 20,
                self.mesh_budget >> 20
            );
            self.cache_warning = true;
        }
        let mut stale: Vec<_> = self
            .sections
            .iter()
            .filter(|(_, s)| !s.gpu.is_empty() && s.last_used != self.frames + 1)
            .map(|(&k, s)| (s.last_used, k))
            .collect();
        stale.sort_unstable();
        let mut evict = stale.into_iter();
        // Limit cache population work, rather than stalling a frame to create
        // thousands of D3D resources. New sections are immediately drawable.
        let started = Instant::now();
        for &(key, _, _) in visible {
            let section = &self.sections[&key];
            if !section.gpu.is_empty()
                || section.vertices.is_empty()
                || self.frames < section.retry_after
            {
                continue;
            }
            let bytes = section.bytes;
            while self.gpu_mesh_bytes + bytes > self.mesh_budget {
                let Some((_, key)) = evict.next() else {
                    break;
                };
                let old = self.sections.get_mut(&key).unwrap();
                self.gpu_mesh_bytes -= old.gpu.iter().map(dx::Mesh::bytes).sum::<u64>();
                old.gpu.clear();
            }
            if self.gpu_mesh_bytes + bytes > self.mesh_budget {
                continue;
            }
            let section = self.sections.get_mut(&key).unwrap();
            let allocation: Result<Vec<_>> = section
                .vertices
                .chunks(self.device.max_vertices())
                .map(|v| self.device.mesh(v))
                .collect();
            match allocation {
                Ok(buffers) => {
                    section.gpu = buffers;
                    self.gpu_mesh_bytes += bytes;
                }
                Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
                Err(error) => {
                    if !self.cache_warning {
                        log::warn!(
                            "D3D9 terrain GPU cache allocation failed ({error}); using transient draws"
                        );
                        self.cache_warning = true;
                    }
                    section.retry_after = self.frames + 120;
                }
            }
            if started.elapsed() >= Duration::from_millis(2) {
                break;
            }
        }
        Ok(())
    }
    pub fn clear_sections(&mut self, layout: ColumnLayout) {
        self.sections.clear();
        self.layout = layout;
        self.mesh_bytes = 0;
        self.gpu_mesh_bytes = 0;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn remove_columns(&mut self, columns: impl IntoIterator<Item = (i32, i32)>) {
        for (x, z) in columns {
            self.sections.retain(|key, s| {
                if key.0 == x && key.2 == z {
                    self.mesh_bytes -= s.bytes;
                    self.gpu_mesh_bytes = self.gpu_mesh_bytes.saturating_sub(
                        s.gpu
                            .iter()
                            .filter(|b| b.resident())
                            .map(dx::Mesh::bytes)
                            .sum::<u64>(),
                    );
                    false
                } else {
                    true
                }
            });
        }
    }
    pub fn has_section(&self, key: SectionKey) -> bool {
        self.sections.contains_key(&key)
    }
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }
    pub fn resident_sections(&self) -> usize {
        self.sections.values().filter(|s| !s.gpu.is_empty()).count()
    }
    pub fn block_entities(&self) -> impl Iterator<Item = &([i32; 3], u32)> {
        self.sections.values().flat_map(|s| &s.blocks)
    }
    pub fn visible_block_entities(
        &self,
        _fade_seconds: f32,
    ) -> impl Iterator<Item = &([i32; 3], u32)> {
        self.block_entities()
    }
    pub fn visible_block_entities_in_frustum<'a>(
        &'a self,
        _fade_seconds: f32,
        camera: DVec3,
        frustum: Option<Frustum>,
        keep_section: impl Fn(u32) -> bool + 'a,
        keep_long_range: impl Fn(u32) -> bool + 'a,
    ) -> impl Iterator<Item = (&'a ([i32; 3], u32), bool)> + 'a {
        let min_y = self.layout.min_y();
        self.sections
            .iter()
            .filter_map(move |(key, section)| {
                if section.blocks.is_empty() {
                    return None;
                }
                let min = (DVec3::new(
                    f64::from(key.0) * 16.0 - 1.0,
                    f64::from(min_y) + f64::from(key.1) * 16.0 - 1.0,
                    f64::from(key.2) * 16.0 - 1.0,
                ) - camera)
                    .as_vec3();
                let max = min + Vec3::splat(18.0);
                let distance = Vec3::ZERO.max(min).min(max).length_squared();
                if distance > 256.0 * 256.0
                    || (distance > 64.0 * 64.0
                        && !section.blocks.iter().any(|(_, id)| keep_long_range(*id)))
                {
                    return None;
                }
                let inside = frustum.is_none_or(|f| f.contains(min, max));
                (inside
                    || frustum.is_none_or(|f| f.intersects(min, max))
                    || section.blocks.iter().any(|(_, id)| keep_section(*id)))
                .then_some((&section.blocks, inside))
            })
            .flat_map(|(blocks, inside)| blocks.iter().map(move |b| (b, inside)))
    }
    pub fn block_entity_revision(&self) -> u64 {
        self.revision
    }
    pub fn block_entity_section_revision(
        &self,
        section: [i32; 3],
    ) -> Option<StaticSectionRevision> {
        self.sections
            .get(&(section[0], section[1], section[2]))
            .map(|s| StaticSectionRevision {
                mesh: s.revision,
                block_entities: s.revision,
            })
    }
}
fn terrain_quad(input: &[TerrainVertex]) -> [dx::Vertex; 4] {
    std::array::from_fn(|i| dx::Vertex {
        pos: input[i].pos,
        uv: input[i].uv(),
        color: dx::rgba_to_argb(input[i].color),
        light: light_uv(u32::from(input[i].light)),
    })
}

pub(super) fn draw_packet(
    device: &mut dx::Device,
    section: &Section,
    packet: &Packet,
) -> Result<()> {
    let limit = device.max_vertices();
    let mut first = packet.first as usize;
    let end = first + packet.count as usize;
    while first < end {
        let buffer = first / limit;
        let offset = first % limit;
        let count = (end - first).min(limit - offset);
        if let Some(mesh) = section.gpu.get(buffer) {
            device.draw(mesh, offset as u32, count as u32)?;
        } else {
            device.draw_transient(&section.vertices[first..first + count])?;
        }
        first += count;
    }
    Ok(())
}
