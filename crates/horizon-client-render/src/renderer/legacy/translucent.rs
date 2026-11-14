//! Reuse Horizon's translucent viewpoint sort; only ordered indices change.
use super::*;
use crate::gfx::translucency::{SortState, Sorter};

pub(super) struct Sorted {
    state: SortState,
    first: u32,
    centers_bytes: usize,
    pub order: Vec<u32>,
    pages: Vec<u32>,
    changed: bool,
    stamp: Vec<(u64, u32, Range<u32>)>,
    buffers: BTreeMap<u64, dx::Indices>,
    pub draws: Vec<Draw>,
}
pub(super) struct Draw {
    pub page: usize,
    pub arena: u64,
    pub group: u32,
}
#[derive(Default)]
pub(super) struct Scratch {
    batches: Vec<(u64, dx::IndexBatch)>,
    used: usize,
}
impl Scratch {
    fn batch(&mut self, arena: u64) -> &mut dx::IndexBatch {
        let at = self.batches[..self.used]
            .iter()
            .position(|b| b.0 == arena)
            .unwrap_or_else(|| {
                let at = self.used;
                if at == self.batches.len() {
                    self.batches.push((arena, dx::IndexBatch::default()));
                }
                self.batches[at].0 = arena;
                self.batches[at].1.clear();
                self.used += 1;
                at
            });
        &mut self.batches[at].1
    }
    pub fn bytes(&self) -> usize {
        self.batches.capacity() * size_of::<(u64, dx::IndexBatch)>()
            + self.batches.iter().map(|b| b.1.bytes()).sum::<usize>()
    }
}
impl Sorted {
    pub fn new(first: u32, centers: Vec<[f32; 3]>, pages: Vec<u32>) -> Self {
        Self {
            centers_bytes: centers.capacity() * size_of::<[f32; 3]>(),
            state: SortState::new(centers),
            first,
            pages,
            order: Vec::new(),
            changed: true,
            stamp: Vec::new(),
            buffers: BTreeMap::new(),
            draws: Vec::new(),
        }
    }
    pub fn bytes(&self) -> usize {
        self.centers_bytes
            + self.pages.capacity() * 4
            + self.order.capacity() * 4
            + self.stamp.capacity() * size_of::<(u64, u32, Range<u32>)>()
            + self.draws.capacity() * size_of::<Draw>()
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.buffers.values().map(dx::Indices::bytes).sum()
    }
    pub fn update(&mut self, camera: DVec3, origin: [i32; 3], sorter: &mut Sorter) {
        if let Some(indices) = self.state.update(camera.to_array(), origin, sorter) {
            self.order.clear();
            self.order
                .extend(indices.chunks_exact(6).map(|q| self.first + q[0]));
            self.changed = true;
        }
    }
    pub fn prepare(
        &mut self,
        device: &mut dx::Device,
        section: &Section,
        scratch: &mut Scratch,
    ) -> Result<()> {
        let same = self.stamp.len() == section.gpu.len()
            && self
                .stamp
                .iter()
                .zip(&section.gpu)
                .all(|(a, b)| a.0 == b.arena && a.1 == b.first && a.2 == b.source);
        if !self.changed && same {
            return Ok(());
        }
        let _span = crate::spans::guard("d3d9_translucent_indices");
        scratch.used = 0;
        self.draws.clear();
        let mut open: Option<(usize, u64, u32, u32, u32)> = None;
        for &source in &self.order {
            let page = self.page(source);
            let slice = section
                .gpu
                .iter()
                .find(|s| s.source.contains(&source))
                .ok_or("D3D9 sorted quad is missing its GPU slice")?;
            let first = slice.first + source - slice.source.start;
            let split = open.is_some_and(|(old_page, arena, lo, hi, count)| {
                old_page != page
                    || arena != slice.arena
                    || (device.caps.software_vertices
                        && hi.max(first + 4) - lo.min(first) > (count + 4) * 2)
            });
            if split {
                Self::flush(&mut open, scratch, &mut self.draws);
            }
            let range = open.get_or_insert((page, slice.arena, first, first + 4, 0));
            range.2 = range.2.min(first);
            range.3 = range.3.max(first + 4);
            range.4 += 4;
            scratch.batch(slice.arena).append(first, 4)?;
        }
        Self::flush(&mut open, scratch, &mut self.draws);
        // Build all replacements before dropping old buffers. An allocation
        // failure retains source geometry for ordered direct/transient draws.
        let mut buffers = BTreeMap::new();
        for (arena, batch) in &scratch.batches[..scratch.used] {
            buffers.insert(*arena, device.cache_indices(batch)?);
        }
        self.buffers = buffers;
        self.stamp.clear();
        self.stamp.extend(
            section
                .gpu
                .iter()
                .map(|s| (s.arena, s.first, s.source.clone())),
        );
        self.changed = false;
        Ok(())
    }
    fn flush(
        open: &mut Option<(usize, u64, u32, u32, u32)>,
        scratch: &mut Scratch,
        draws: &mut Vec<Draw>,
    ) {
        if let Some((page, arena, _, _, _)) = open.take()
            && let Some(group) = scratch.batch(arena).finish_group()
        {
            draws.push(Draw { page, arena, group });
        }
    }
    pub fn draw(
        &self,
        device: &mut dx::Device,
        arenas: &HashMap<u64, Arena>,
        draw: &Draw,
    ) -> Result<()> {
        device.draw_cached_group(
            &arenas[&draw.arena].mesh,
            &self.buffers[&draw.arena],
            draw.group,
        )
    }
    pub fn page(&self, first: u32) -> usize {
        self.pages[((first - self.first) / 4) as usize] as usize
    }
}
