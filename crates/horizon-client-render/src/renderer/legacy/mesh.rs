use super::*;
mod compact;
pub(super) use compact::Compactor;
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
    pub vertices: Arc<dx::MeshData>,
    pub gpu: Vec<GpuSlice>,
    pub retry_after: u64,
    pub sides: crate::mesher::Sides,
    pub blocks: Vec<([i32; 3], u32)>,
    pub revision: u64,
    pub bytes: u64,
}
pub(super) type RegionKey = (i32, i32, i32);
pub(super) struct GpuSlice {
    pub arena: u64,
    pub first: u32,
    pub source: Range<u32>,
}
pub(super) struct Arena {
    pub mesh: dx::Mesh,
    pub region: RegionKey,
    pub live: u32,
    pub last_used: u64,
    pub sections: Vec<SectionKey>,
}
pub(super) fn region_key(key: SectionKey) -> RegionKey {
    (
        key.0.div_euclid(8),
        key.1.div_euclid(8),
        key.2.div_euclid(8),
    )
}
pub(super) fn region_origin(region: RegionKey, layout: ColumnLayout, camera: DVec3) -> Vec3 {
    (DVec3::new(
        f64::from(region.0) * 128.0,
        f64::from(region.1) * 128.0 + f64::from(layout.min_y()),
        f64::from(region.2) * 128.0,
    ) - camera)
        .as_vec3()
}
struct PendingArena {
    region: RegionKey,
    vertices: Vec<dx::Vertex>,
    sections: Vec<SectionKey>,
}
#[derive(Default)]
pub(super) struct DrawBatch {
    pub next: Vec<(u32, u32)>,
    ranges: Vec<(u32, u32)>,
    indices: Option<dx::Indices>,
}
impl DrawBatch {
    pub fn append(&mut self, first: u32, count: u32) {
        if let Some(last) = self.next.last_mut()
            && last.0 + last.1 == first
        {
            last.1 += count;
        } else {
            self.next.push((first, count));
        }
    }
    pub fn bytes(&self) -> usize {
        (self.next.capacity() + self.ranges.capacity()) * size_of::<(u32, u32)>()
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.indices.as_ref().map_or(0, dx::Indices::bytes)
    }
    pub fn draw(
        &mut self,
        device: &mut dx::Device,
        mesh: &dx::Mesh,
        scratch: &mut dx::IndexBatch,
    ) -> Result<()> {
        if self.next.is_empty() {
            return Ok(());
        }
        // Visibility membership matters; near-first ordering of opaque sections
        // does not. Canonical ranges keep camera motion from rewriting the IB.
        self.next.sort_unstable();
        let mut used = 0;
        for i in 0..self.next.len() {
            let (first, count) = self.next[i];
            if used > 0 && self.next[used - 1].0 + self.next[used - 1].1 == first {
                self.next[used - 1].1 += count;
            } else {
                self.next[used] = (first, count);
                used += 1;
            }
        }
        self.next.truncate(used);
        if self.next.len() == 1 {
            self.indices = None;
            self.ranges.clear();
            let (first, count) = self.next[0];
            return device.draw(mesh, first, count);
        }
        if self.ranges != self.next || self.indices.is_none() {
            scratch.clear();
            for &(first, count) in &self.next {
                scratch.append(first, count)?;
            }
            self.indices = Some(device.cache_indices(scratch)?);
            self.ranges.clone_from(&self.next);
        }
        device.draw_cached(mesh, self.indices.as_ref().unwrap())
    }
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
        // Bucket/page/alpha indices are dense and fixed by this resource set.
        // Reuse their vectors instead of allocating tree nodes and buffers per section.
        let pages = self.sprites.pages.len();
        self.upload_groups.resize_with(7 * pages * 2, Vec::new);
        for group in &mut self.upload_groups {
            group.clear();
        }
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
            self.sprites.quad(
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &terrain_quad(input),
                |page, poly| {
                    let opaque = input[0].layer & crate::mesher::OPAQUE_LAYER != 0;
                    let group =
                        &mut self.upload_groups[(bucket * pages + page) * 2 + usize::from(opaque)];
                    atlas::quads(poly, group);
                },
            )?;
        }
        let mut vertices = std::mem::take(&mut self.upload_vertices);
        vertices.clear();
        vertices.reserve(mesh.solid.len() + mesh.translucent.len());
        let mut solid = Vec::new();
        for (i, part) in self
            .upload_groups
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
        {
            let page = i / 2 % pages;
            let bucket = i / (2 * pages);
            let source = thresholds.map_or(0..mesh.solid.len() as u32 / 4, |ends| {
                if bucket == 0 {
                    0..ends[0]
                } else {
                    ends[bucket - 1]..ends[bucket]
                }
            });
            solid.push(Packet {
                page,
                opaque: i % 2 != 0,
                first: vertices.len() as u32,
                count: part.len() as u32,
                source,
            });
            vertices.extend_from_slice(part);
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
        let offset = [
            (key.0.rem_euclid(8) * 16) as f32,
            (key.1.rem_euclid(8) * 16) as f32,
            (key.2.rem_euclid(8) * 16) as f32,
        ];
        for v in &mut vertices {
            for axis in 0..3 {
                v.pos[axis] += offset[axis];
            }
        }
        let decoded_bytes = vertices.len() as u64 * size_of::<dx::Vertex>() as u64;
        let shadow = Arc::new(dx::MeshData::with_scratch(
            &vertices,
            &mut self.shadow_scratch,
        ));
        let bytes = shadow.bytes();
        self.release_gpu_section(key);
        if let Some(old) = self.sections.remove(&key) {
            self.mesh_bytes -= old.bytes;
            self.decoded_mesh_bytes -= old.vertices.len() as u64 * size_of::<dx::Vertex>() as u64;
        }
        self.revision = self.revision.wrapping_add(1);
        self.sections.insert(
            key,
            Section {
                solid,
                translucent,
                vertices: shadow,
                gpu: Vec::new(),
                retry_after: 0,
                sides,
                blocks: mesh.block_entities.clone(),
                revision: self.revision,
                bytes,
            },
        );
        self.mesh_bytes += bytes;
        self.decoded_mesh_bytes += decoded_bytes;
        self.occlusion.set(key, mesh.visibility);
        if decoded_bytes != 0 {
            self.spatial.insert_marked(key, (), mesh.bounds, false);
        } else {
            self.spatial.remove(key);
        }
        self.prefetch_ready_section(key, &vertices);
        self.upload_vertices = vertices;
        Ok(())
    }
    fn prefetch_ready_section(&mut self, key: SectionKey, vertices: &[dx::Vertex]) {
        let bytes = std::mem::size_of_val(vertices) as u64;
        if vertices.is_empty()
            || self.device.lost()
            || self.occlusion.reach().is_none()
            || !self.occlusion.contains(key)
            || self.gpu_mesh_bytes + bytes > self.mesh_budget
        {
            return;
        }
        // The expanded vertices already exist at a section update. Upload now,
        // including behind the camera, instead of decompressing them on first view.
        let mut pages = Vec::new();
        for (i, part) in vertices.chunks(self.device.max_vertices()).enumerate() {
            match self.device.mesh(part) {
                Ok(mesh) => {
                    self.pending_mesh_uploads += mesh.bytes();
                    let first = (i * self.device.max_vertices()) as u32;
                    pages.push((mesh, first..first + part.len() as u32));
                }
                Err(error) => {
                    if dx::is_device_lost(error.as_ref()) {
                        self.device.mark_lost();
                    } else if !self.cache_warning {
                        log::warn!(
                            "D3D9 eager upload: {error}; keeping CPU shadow for demand loading"
                        );
                        self.cache_warning = true;
                    }
                    return;
                }
            }
        }
        for (mesh, source) in pages {
            self.next_arena += 1;
            let id = self.next_arena;
            self.gpu_mesh_bytes += mesh.bytes();
            self.arenas.insert(
                id,
                Arena {
                    mesh,
                    region: region_key(key),
                    live: source.end - source.start,
                    last_used: self.frames,
                    sections: vec![key],
                },
            );
            self.sections.get_mut(&key).unwrap().gpu.push(GpuSlice {
                arena: id,
                first: 0,
                source,
            });
        }
        self.eager_sections += 1;
    }
    pub(super) fn release_gpu_section(&mut self, key: SectionKey) {
        let Some(section) = self.sections.get_mut(&key) else {
            return;
        };
        let slices = std::mem::take(&mut section.gpu);
        let mut empty = Vec::new();
        for slice in slices {
            if let Some(arena) = self.arenas.get_mut(&slice.arena) {
                arena.live -= slice.source.end - slice.source.start;
                if arena.live == 0 {
                    empty.push(slice.arena);
                }
            }
        }
        for id in empty {
            if let Some(arena) = self.arenas.remove(&id) {
                self.gpu_mesh_bytes -= arena.mesh.bytes();
            }
        }
    }
    pub(super) fn prune_hidden(&mut self, camera: DVec3) {
        // A generous RTX cache must not retain caves merely uploaded while the
        // asynchronous portal graph was still conservative during world loading.
        // Throttle the census; unknown/new-camera graphs never discard anything.
        if !self.frames.is_multiple_of(32) {
            return;
        }
        let from = (
            (camera.x.floor() as i32) >> 4,
            (camera.y.floor() as i32 - self.layout.min_y()) >> 4,
            (camera.z.floor() as i32) >> 4,
        );
        let hidden: Vec<_> = self
            .sections
            .iter()
            .filter_map(|(&key, section)| {
                (!section.gpu.is_empty()
                    && self.occlusion.visible_for_camera(key, from) == Some(false))
                .then_some(key)
            })
            .collect();
        for key in hidden {
            self.release_gpu_section(key);
        }
    }
    fn evict_arena(&mut self, id: u64) {
        if let Some(arena) = self.arenas.remove(&id) {
            self.gpu_mesh_bytes -= arena.mesh.bytes();
            for key in arena.sections {
                if self
                    .sections
                    .get(&key)
                    .is_some_and(|s| s.gpu.iter().any(|slice| slice.arena == id))
                {
                    self.release_gpu_section(key);
                }
            }
        }
    }
    fn cache_section(
        &mut self,
        key: SectionKey,
        pending: &mut HashMap<u64, PendingArena>,
        pending_bytes: &mut u64,
        stale: &mut impl Iterator<Item = (u64, u64)>,
    ) -> Result<bool> {
        let bytes = self.sections[&key].vertices.len() as u64 * size_of::<dx::Vertex>() as u64;
        while self.gpu_mesh_bytes + *pending_bytes + bytes > self.mesh_budget {
            let Some((_, id)) = stale.next() else {
                return Ok(false);
            };
            if self
                .arenas
                .get(&id)
                .is_some_and(|a| a.last_used != self.frames + 1)
            {
                self.evict_arena(id);
            }
        }
        let region = region_key(key);
        let mut decoded = std::mem::take(&mut self.decode_scratch);
        self.sections[&key].vertices.decode(&mut decoded)?;
        let limit = self.device.max_vertices();
        let mut first = 0;
        while first < decoded.len() {
            let id = if let Some((&id, _)) = pending
                .iter()
                .find(|(_, a)| a.region == region && a.vertices.len() < limit)
            {
                id
            } else {
                self.next_arena += 1;
                let id = self.next_arena;
                pending.insert(
                    id,
                    PendingArena {
                        region,
                        vertices: Vec::new(),
                        sections: Vec::new(),
                    },
                );
                id
            };
            let arena = pending.get_mut(&id).unwrap();
            let count = (limit - arena.vertices.len()).min(decoded.len() - first);
            let offset = arena.vertices.len() as u32;
            arena
                .vertices
                .extend_from_slice(&decoded[first..first + count]);
            if arena.sections.last() != Some(&key) {
                arena.sections.push(key);
            }
            self.sections.get_mut(&key).unwrap().gpu.push(GpuSlice {
                arena: id,
                first: offset,
                source: first as u32..(first + count) as u32,
            });
            *pending_bytes += count as u64 * size_of::<dx::Vertex>() as u64;
            first += count;
        }
        self.decode_scratch = decoded;
        Ok(true)
    }
    /// Compressed CPU shadows survive Reset. Shared GPU arenas are immutable:
    /// no in-flight data is overwritten, and indices alone change with visibility.
    pub(super) fn prepare_sections(&mut self, visible: &[gfx::resident::Visible]) -> Result<()> {
        if self.seen_resets != self.device.resets
            || self.arenas.values().any(|a| !a.mesh.resident())
        {
            self.seen_resets = self.device.resets;
            self.arenas.clear();
            self.gpu_mesh_bytes = 0;
            for section in self.sections.values_mut() {
                section.gpu.clear();
                section.retry_after = 0;
            }
        }
        self.collect_compaction()?;
        for v in visible {
            let key = &v.key;
            for slice in &self.sections[key].gpu {
                self.arenas.get_mut(&slice.arena).unwrap().last_used = self.frames + 1;
            }
        }
        let mut stale: Vec<_> = self
            .arenas
            .iter()
            .filter(|(_, a)| a.last_used != self.frames + 1)
            .map(|(&id, a)| (a.last_used, id))
            .collect();
        stale.sort_unstable();
        let mut stale = stale.into_iter();
        let mut pending = HashMap::<u64, PendingArena>::default();
        let mut pending_bytes = 0;
        for v in visible {
            let key = v.key;
            let section = &self.sections[&key];
            if !section.gpu.is_empty()
                || section.vertices.is_empty()
                || self.frames < section.retry_after
            {
                continue;
            }
            match self.cache_section(key, &mut pending, &mut pending_bytes, &mut stale) {
                Ok(true) => {}
                Ok(false) => {
                    self.sections.get_mut(&key).unwrap().retry_after = self.frames + 30;
                }
                Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
                Err(error) => {
                    if !self.cache_warning {
                        log::warn!(
                            "D3D9 arena allocation failed ({error}); using bounded transient draws"
                        );
                        self.cache_warning = true;
                    }
                    self.sections.get_mut(&key).unwrap().retry_after = self.frames + 120;
                }
            }
        }
        // Upload each new region page once as immutable DEFAULT storage. Dynamic
        // terrain VBs can live in AGP/system RAM on discrete GPUs; static pages
        // avoid that extra residency and never lock data already used by a draw.
        let pending_keys: Vec<_> = pending
            .values()
            .flat_map(|p| p.sections.iter().copied())
            .collect();
        for (id, page) in pending {
            let live: u32 = page
                .sections
                .iter()
                .filter_map(|k| self.sections.get(k))
                .flat_map(|s| &s.gpu)
                .filter(|s| s.arena == id)
                .map(|s| s.source.end - s.source.start)
                .sum();
            if live == 0 {
                continue;
            }
            match self.device.mesh(&page.vertices) {
                Ok(mesh) => {
                    self.gpu_mesh_bytes += mesh.bytes();
                    self.arenas.insert(
                        id,
                        Arena {
                            mesh,
                            region: page.region,
                            live,
                            last_used: self.frames + 1,
                            sections: page.sections,
                        },
                    );
                }
                Err(error) => {
                    for key in page.sections {
                        self.release_gpu_section(key);
                        if let Some(s) = self.sections.get_mut(&key) {
                            s.retry_after = self.frames + 120;
                        }
                    }
                    if dx::is_device_lost(error.as_ref()) {
                        for key in pending_keys {
                            self.release_gpu_section(key);
                        }
                        return Err(error);
                    }
                    if !self.cache_warning {
                        log::warn!(
                            "D3D9 static arena allocation failed ({error}); using transient draws"
                        );
                        self.cache_warning = true;
                    }
                }
            }
        }
        self.solid_batches
            .retain(|(_, _, id), _| self.arenas.contains_key(id));
        for batch in self.solid_batches.values_mut() {
            batch.next.clear();
        }
        self.schedule_compaction();
        Ok(())
    }
    pub fn clear_sections(&mut self, layout: ColumnLayout) {
        self.sections.clear();
        self.spatial.clear();
        self.spatial.set_min_y(layout.min_y());
        if self.layout != layout {
            self.occlusion =
                gfx::occlusion::Occlusion::new(layout.min_y(), layout.sections() as i32);
        } else {
            self.occlusion.clear();
        }
        self.visible.clear();
        self.arenas.clear();
        self.solid_batches.clear();
        self.decode_scratch.clear();
        self.decoded_mesh_bytes = 0;
        self.layout = layout;
        self.mesh_bytes = 0;
        self.gpu_mesh_bytes = 0;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn remove_columns(&mut self, columns: impl IntoIterator<Item = (i32, i32)>) {
        let columns: FxHashSet<_> = columns.into_iter().collect();
        self.occlusion.remove_columns(&columns);
        let keys: Vec<_> = self
            .sections
            .keys()
            .copied()
            .filter(|k| columns.contains(&(k.0, k.2)))
            .collect();
        for key in keys {
            self.spatial.remove(key);
            self.release_gpu_section(key);
            if let Some(s) = self.sections.remove(&key) {
                self.mesh_bytes -= s.bytes;
                self.decoded_mesh_bytes -= s.vertices.len() as u64 * size_of::<dx::Vertex>() as u64;
            }
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
    arenas: &HashMap<u64, Arena>,
    section: &Section,
    packet: &Packet,
    decoded: &[dx::Vertex],
) -> Result<()> {
    if section.gpu.is_empty() {
        return device.draw_transient(
            &decoded[packet.first as usize..(packet.first + packet.count) as usize],
        );
    }
    for slice in &section.gpu {
        let first = packet.first.max(slice.source.start);
        let end = (packet.first + packet.count).min(slice.source.end);
        if first < end {
            device.draw(
                &arenas[&slice.arena].mesh,
                slice.first + first - slice.source.start,
                end - first,
            )?;
        }
    }
    Ok(())
}
