use super::*;
use crate::block_entity_render::StaticSectionSnapshot;

const MAX_SECTIONS: usize = 256;
const BUILDS_PER_FRAME: usize = 4;
const UPLOAD_BYTES_PER_FRAME: usize = 512 << 10;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Context {
    reset: u64,
    atlas_presence: u64,
    nether_lighting: bool,
}

struct Section {
    revision: u64,
    // None records an unsupported section or a failed optional allocation.
    buffer: Option<Buffer>,
}

struct Buffer {
    mesh: dx::Mesh,
    ranges: Box<[DrawRange]>,
}

struct DrawRange {
    page: usize,
    state: DrawState,
    first: u32,
    count: u32,
}

impl Buffer {
    fn bytes(&self) -> usize {
        self.mesh.bytes() as usize + std::mem::size_of_val(self.ranges.as_ref())
    }
}

pub(in crate::renderer::legacy) struct Cache {
    sections: HashMap<[i32; 3], Section>,
    context: Option<Context>,
    catalog: Option<u64>,
    budget: usize,
    bytes: usize,
    uploads_left: usize,
    builds_left: usize,
    hits: u64,
    misses: u64,
    fallbacks: u64,
    draws: u64,
    uploaded_bytes: u64,
    warned: bool,
}

impl Cache {
    pub fn new(budget: usize) -> Self {
        Self {
            sections: HashMap::default(),
            context: None,
            catalog: None,
            budget,
            bytes: 0,
            uploads_left: 0,
            builds_left: 0,
            hits: 0,
            misses: 0,
            fallbacks: 0,
            draws: 0,
            uploaded_bytes: 0,
            warned: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.budget != 0
    }

    pub fn clear(&mut self) {
        self.sections.clear();
        self.context = None;
        self.catalog = None;
        self.bytes = 0;
    }

    pub fn begin(&mut self, frame: &Frame, reset: u64, atlas_presence: u64) {
        let context = Context {
            reset,
            atlas_presence,
            nether_lighting: frame.nether_lighting,
        };
        if self.context != Some(context) {
            self.clear();
            self.context = Some(context);
        }
        self.retain_catalog(
            frame.static_block_entity_stats.snapshot_revision,
            frame.static_block_entities,
        );
        self.uploads_left = UPLOAD_BYTES_PER_FRAME;
        self.builds_left = BUILDS_PER_FRAME;
    }

    fn retain_catalog(&mut self, revision: u64, snapshots: &[StaticSectionSnapshot]) {
        if self.catalog == Some(revision) {
            return;
        }
        // Published snapshots are sorted by section. Drop unloaded and changed
        // buffers without walking the catalog while its revision is stable.
        self.sections.retain(|key, entry| {
            snapshots
                .binary_search_by_key(key, |s| s.section)
                .is_ok_and(|at| snapshots[at].revision == entry.revision)
        });
        self.bytes = self
            .sections
            .values()
            .filter_map(|s| s.buffer.as_ref())
            .map(Buffer::bytes)
            .sum();
        self.catalog = Some(revision);
    }

    fn can_build(&self, section: &StaticSectionSnapshot, capacity: usize) -> bool {
        let vertices = section.vertices.len();
        self.builds_left != 0
            && self.sections.len() < MAX_SECTIONS
            && !self.sections.contains_key(&section.section)
            && (4..=capacity).contains(&vertices)
            && vertices * size_of::<dx::Vertex>() <= self.uploads_left
            && vertices * size_of::<dx::Vertex>() <= self.budget - self.bytes
    }

    pub fn vertex_bytes(&self) -> u64 {
        self.sections
            .values()
            .filter_map(|s| s.buffer.as_ref())
            .map(|b| b.mesh.bytes())
            .sum()
    }

    pub fn cpu_bytes(&self) -> usize {
        self.sections.capacity() * size_of::<([i32; 3], Section)>() + self.bytes
    }

    pub fn log(&self) {
        if self.enabled() {
            log::info!(
                "D3D9 static entity cache entries={} hits_total={} misses_total={} fallback_total={} draws_total={} upload_bytes_total={} retained_bytes={}",
                self.sections.len(),
                self.hits,
                self.misses,
                self.fallbacks,
                self.draws,
                self.uploaded_bytes,
                self.bytes
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_change_retires_changed_and_unloaded_sections() {
        let mut cache = Cache::new(8 << 20);
        for x in 0..3 {
            cache.sections.insert(
                [x, 0, 0],
                Section {
                    revision: 1,
                    buffer: None,
                },
            );
        }
        let snapshot = |x, revision| StaticSectionSnapshot {
            section: [x, 0, 0],
            origin: [x * 16, 0, 0],
            revision,
            entities: 0,
            vertices: Arc::from([]),
        };
        cache.retain_catalog(2, &[snapshot(0, 1), snapshot(1, 2)]);
        assert_eq!(
            cache.sections.keys().copied().collect::<Vec<_>>(),
            vec![[0, 0, 0]]
        );
    }
}

impl Renderer {
    pub(super) fn draw_static_entity_section(
        &mut self,
        frame: &Frame,
        section: &StaticSectionSnapshot,
        fog: Option<dx::Fog>,
    ) -> Result<()> {
        if !self.static_entities.enabled() {
            return self.draw_entities(frame, &section.vertices, fog, 2);
        }

        let build = self
            .static_entities
            .can_build(section, self.device.max_vertices());
        let mut uncached = None;
        if build {
            let _span = crate::spans::guard("d3d9_static_entity_build");
            self.static_entities.builds_left -= 1;
            self.static_entities.misses += 1;
            // Plain block-entity quads depend only on the snapshot, light
            // direction and atlas layout. Animated overlays take the live path.
            if section
                .vertices
                .chunks_exact(4)
                .all(|quad| quad[0].layer & 0xffff == 0)
            {
                let batches = self.entity_batches(frame, &section.vertices, fog, 2)?;
                debug_assert!(batches.sprites.is_empty() && batches.glints.is_empty());
                self.retain_static_entity_buffer(section, &batches.pixels)?;
                uncached = Some(batches.pixels);
            } else {
                self.static_entities.sections.insert(
                    section.section,
                    Section {
                        revision: section.revision,
                        buffer: None,
                    },
                );
            }
        }

        let Some(buffer) = self
            .static_entities
            .sections
            .get(&section.section)
            .filter(|entry| entry.revision == section.revision)
            .and_then(|entry| entry.buffer.as_ref())
            .filter(|buffer| buffer.mesh.resident())
        else {
            self.static_entities.fallbacks += 1;
            let _span = crate::spans::guard("d3d9_static_entity_fallback");
            if let Some(batches) = uncached {
                return draw_pixels(
                    &mut self.device,
                    &self.hud.atlas,
                    &self.lightmap,
                    &batches,
                    false,
                );
            }
            return self.draw_entities(frame, &section.vertices, fog, 2);
        };

        let _span = crate::spans::guard("d3d9_static_entity_submit");
        self.static_entities.hits += u64::from(!build);
        self.hud.atlas.prepare(&mut self.device)?;
        let textures = self.hud.atlas.textures();
        let before = self.device.draws;
        for range in &buffer.ranges {
            let Some(texture) = &textures[range.page] else {
                continue;
            };
            self.device.state(
                DrawState {
                    fog,
                    linear: self.linear,
                    ..range.state
                },
                texture,
                Some(&self.lightmap),
            )?;
            self.device.draw(&buffer.mesh, range.first, range.count)?;
        }
        self.static_entities.draws += self.device.draws - before;
        Ok(())
    }

    fn retain_static_entity_buffer(
        &mut self,
        section: &StaticSectionSnapshot,
        batches: &[Batch],
    ) -> Result<()> {
        let limit = self
            .device
            .max_vertices()
            .min(UPLOAD_BYTES_PER_FRAME / size_of::<dx::Vertex>());
        let vertices = batches.iter().try_fold(0usize, |n, batch| {
            n.checked_add(batch.vertices.len())
                .filter(|&count| count <= limit)
        });
        let cache = &mut self.static_entities;
        let Some(vertices) = vertices else {
            cache.sections.insert(
                section.section,
                Section {
                    revision: section.revision,
                    buffer: None,
                },
            );
            return Ok(());
        };
        let bytes = vertices * size_of::<dx::Vertex>();
        let retained = bytes + batches.len() * size_of::<DrawRange>();
        if bytes > cache.uploads_left || retained > cache.budget - cache.bytes {
            return Ok(());
        }
        let mut buffer = None;
        if vertices != 0 {
            self.entity_stage_vertices.clear();
            let mut ranges = Vec::with_capacity(batches.len());
            for batch in batches {
                ranges.push(DrawRange {
                    page: batch.page,
                    state: DrawState {
                        fog: None,
                        ..batch.state
                    },
                    first: self.entity_stage_vertices.len() as u32,
                    count: batch.vertices.len() as u32,
                });
                self.entity_stage_vertices
                    .extend_from_slice(&batch.vertices);
            }
            let mesh = self.device.mesh(&self.entity_stage_vertices);
            self.entity_stage_vertices.clear();
            match mesh {
                Ok(mesh) => {
                    cache.bytes += retained;
                    cache.uploaded_bytes += bytes as u64;
                    cache.uploads_left -= bytes;
                    buffer = Some(Buffer {
                        mesh,
                        ranges: ranges.into_boxed_slice(),
                    });
                }
                Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
                Err(error) if !cache.warned => {
                    log::warn!(
                        "D3D9 static entity buffer unavailable ({error}); drawing the section transiently"
                    );
                    cache.warned = true;
                }
                Err(_) => {}
            }
        }
        cache.sections.insert(
            section.section,
            Section {
                revision: section.revision,
                buffer,
            },
        );
        Ok(())
    }
}
