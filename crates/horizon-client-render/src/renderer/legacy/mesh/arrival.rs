//! Share immutable pages at first upload, before per-section VBs can fragment
//! the scene. Both hardware encodings and HAL software-vertex FFP use this path.
use super::*;
type Arrivals = dx::UploadBatch<(SectionKey, u64), RegionKey>;

impl Renderer {
    pub(super) fn queue_arrival(
        &self,
        batch: &mut Arrivals,
        key: SectionKey,
        gpu: &dx::EncodedVertices,
    ) -> Result<()> {
        if gpu.is_empty()
            || self.device.lost()
            || self.occlusion.reach().is_some() && !self.occlusion.contains(key)
            || self.gpu_mesh_bytes + batch.bytes() + gpu.bytes().len() as u64 > self.mesh_budget
        {
            return Ok(());
        }
        batch.push(region_key(key), (key, self.sections[&key].revision), gpu)?;
        Ok(())
    }
    pub(super) fn commit_arrivals(&mut self, batch: Arrivals) {
        let mut uploaded = FxHashSet::default();
        for page in batch.into_pages() {
            let parts: Vec<_> = page
                .parts
                .into_iter()
                .filter(|part| {
                    // A large section may already own an earlier page in this
                    // same batch. Its revision, not an empty GPU list, is the guard.
                    self.sections
                        .get(&part.key.0)
                        .is_some_and(|section| section.revision == part.key.1)
                })
                .collect();
            if parts.is_empty() {
                continue;
            }
            let mesh = match self.device.mesh_encoded(&page.vertices) {
                Ok(mesh) => mesh,
                Err(error) => {
                    if dx::is_device_lost(error.as_ref()) {
                        self.device.mark_lost();
                        break;
                    } else if !self.cache_warning {
                        log::warn!(
                            "D3D9 arrival page: {error}; keeping CPU shadows for demand loading"
                        );
                        self.cache_warning = true;
                    }
                    continue;
                }
            };
            self.next_arena += 1;
            let id = self.next_arena;
            let mut keys = Vec::new();
            let mut live = 0;
            for part in parts {
                let key = part.key.0;
                live += part.source.end - part.source.start;
                self.sections.get_mut(&key).unwrap().gpu.push(GpuSlice {
                    arena: id,
                    first: part.first,
                    source: part.source,
                });
                if keys.last() != Some(&key) {
                    keys.push(key);
                }
                uploaded.insert(key);
            }
            self.arrival_pages += 1;
            self.arrival_max_sections = self.arrival_max_sections.max(keys.len());
            self.pending_mesh_uploads += mesh.bytes();
            self.gpu_mesh_bytes += mesh.bytes();
            self.arenas.insert(
                id,
                Arena {
                    mesh,
                    region: page.region,
                    live,
                    last_used: self.frames,
                    sections: keys,
                },
            );
        }
        // Nonempty GPU slices mean complete residency everywhere else. A failed
        // tail page must therefore discard the section's earlier pieces too;
        // its full CPU shadow remains available for demand/transient rendering.
        let partial: Vec<_> = uploaded
            .iter()
            .copied()
            .filter(|key| {
                self.sections.get(key).is_none_or(|section| {
                    section
                        .gpu
                        .iter()
                        .map(|slice| slice.source.len())
                        .sum::<usize>()
                        != section.vertices.len()
                })
            })
            .collect();
        for key in partial {
            self.release_gpu_section(key);
            uploaded.remove(&key);
        }
        self.eager_sections += uploaded.len() as u64;
        self.arrival_sections += uploaded.len() as u64;
    }
}
