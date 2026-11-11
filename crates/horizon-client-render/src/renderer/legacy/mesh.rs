use super::*;
pub(super) struct Packet {
    pub page: usize,
    pub opaque: bool,
    pub mesh: dx::Mesh,
    pub source: Range<u32>,
}
pub(super) struct Section {
    pub solid: Vec<Packet>,
    pub translucent: Vec<Packet>,
    pub sides: crate::mesher::Sides,
    pub bounds: Option<[Vec3; 2]>,
    pub blocks: Vec<([i32; 3], u32)>,
    pub revision: u64,
    pub bytes: u64,
}
impl Renderer {
    pub fn upload(&mut self, key: SectionKey, mesh: &SectionMesh) {
        if let Err(error) = self.upload_section(key, mesh) {
            log::error!("D3D9 section {key:?}: {error}");
            self.pending_error = Some(format!("D3D9 section {key:?}: {error}"));
        }
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
            let vertices = terrain_quad(input);
            self.sprites.quad(
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &vertices,
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
        let solid_bytes: u64 = groups
            .values()
            .map(|(_, v)| v.len() as u64 * size_of::<dx::Vertex>() as u64)
            .sum();
        let mut translucent = Vec::<(usize, Vec<dx::Vertex>)>::new();
        for input in mesh.translucent.chunks_exact(4) {
            self.sprites.quad(
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &terrain_quad(input),
                |page, poly| {
                    if translucent.last().is_none_or(|(p, _)| *p != page) {
                        translucent.push((page, Vec::new()));
                    }
                    if let Some((_, out)) = translucent.last_mut() {
                        atlas::quads(poly, out);
                    }
                },
            )?;
        }
        let bytes = solid_bytes
            + translucent
                .iter()
                .map(|(_, v)| v.len() as u64 * size_of::<dx::Vertex>() as u64)
                .sum::<u64>();
        let old_bytes = self.sections.get(&key).map_or(0, |s| s.bytes);
        if self.mesh_bytes - old_bytes + bytes > self.mesh_budget {
            return Err(format!("section cache would exceed {} MiB (resident {} MiB). Adjust render distance or HORIZON_D3D9_MESH_MIB; no option was rewritten", self.mesh_budget >> 20, self.mesh_bytes >> 20).into());
        }
        // Release the replaced buffers before allocating their successors, bounding peak residency.
        self.sections.remove(&key);
        self.mesh_bytes -= old_bytes;
        let mut upload = |page: usize,
                          opaque: bool,
                          source: Range<u32>,
                          vertices: &[dx::Vertex],
                          out: &mut Vec<Packet>|
         -> Result<()> {
            for chunk in vertices.chunks(self.device.max_vertices()) {
                if !chunk.is_empty() {
                    out.push(Packet {
                        page,
                        opaque,
                        source: source.clone(),
                        mesh: self.device.mesh(chunk)?,
                    });
                }
            }
            Ok(())
        };
        let mut solid = Vec::new();
        for ((_, page, opaque), (range, vertices)) in groups {
            upload(page, opaque, range, &vertices, &mut solid)?;
        }
        let mut transparent = Vec::new();
        for (page, vertices) in translucent {
            upload(page, false, 0..u32::MAX, &vertices, &mut transparent)?;
        }
        self.revision = self.revision.wrapping_add(1);
        self.sections.insert(
            key,
            Section {
                solid,
                translucent: transparent,
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
    pub fn clear_sections(&mut self, layout: ColumnLayout) {
        self.sections.clear();
        self.layout = layout;
        self.mesh_bytes = 0;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn remove_columns(&mut self, columns: impl IntoIterator<Item = (i32, i32)>) {
        for (x, z) in columns {
            self.sections.retain(|key, s| {
                if key.0 == x && key.2 == z {
                    self.mesh_bytes -= s.bytes;
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
        self.sections.len()
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
