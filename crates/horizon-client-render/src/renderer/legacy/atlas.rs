use super::*;
use std::cell::{Ref, RefCell};
mod beam;

pub(super) struct Sprites {
    pub layout: Arc<dx::AtlasSet>,
    pub pages: Vec<dx::Texture>,
    pub animations: Vec<(SpriteAnimation, usize)>,
    mips: Vec<crate::mipmap::Settings>,
    pub frame_cache: dx::AnimationCache,
    frame_scratch: Vec<u8>,
    minimum_size: u32,
    mipmap_option: Option<u32>,
}
impl Sprites {
    pub(super) fn honey_layers(&self, layers: &BTreeMap<String, u32>) -> Option<[u16; 3]> {
        let layer = |name| u16::try_from(*layers.get(name)?).ok();
        let ids = [
            layer("minecraft:block/honey_block_bottom")?,
            layer("minecraft:block/honey_block_side")?,
            layer("minecraft:block/honey_block_top")?,
        ];
        let page = |id| self.layout.transform(u32::from(id)).ok().map(|t| t.page);
        let shell_page = page(ids[0])?;
        // Preserve the subsection's existing page order. A replacement pack
        // can put differently sized honey sprites onto different pages.
        (page(ids[1]) == Some(shell_page) && page(ids[2]) == Some(shell_page)).then_some(ids)
    }

    pub fn new(
        device: &mut dx::Device,
        pixels: &[u8],
        size: u32,
        minimum_size: u32,
        layers: u32,
        settings: &[crate::mipmap::Settings],
        animations: &[SpriteAnimation],
    ) -> Result<Self> {
        if pixels.len() != size as usize * size as usize * 4 * layers as usize {
            return Err("D3D9 sprite pixels do not match their declared size/layers".into());
        }
        let sizes: Vec<_> = (0..layers as usize)
            .map(|i| {
                let native = settings.get(i).map_or(0, |s| s.native_size);
                if native == 0 {
                    size
                } else {
                    native.min(size).next_power_of_two()
                }
            })
            .collect();
        let animated: Vec<_> = animations.iter().map(|a| a.layer).collect();
        let layout = Arc::new(dx::AtlasSet::new(
            &sizes,
            &animated,
            device.caps.max_texture,
        )?);
        let mut pages = Vec::new();
        for (group, plan) in layout.layouts.iter().enumerate() {
            let mut sources = vec![0; plan.layers as usize];
            for (layer, &(g, local)) in layout.locations.iter().enumerate() {
                if g == group {
                    sources[local as usize] = layer;
                }
            }
            let per_page = plan.columns * plan.columns;
            // Use the same native-size, linear-light, alpha-coverage mip chains
            // as the main renderer, including solidified RGB behind cutout alpha.
            let chains: Vec<_> = sources
                .iter()
                .map(|&layer| {
                    let start = layer * size as usize * size as usize * 4;
                    sprite_mips(
                        &pixels[start..start + size as usize * size as usize * 4],
                        settings.get(layer).copied().unwrap_or_default(),
                    )
                })
                .collect();
            for page in 0..plan.pages {
                let mut texture = None;
                for level in 0..plan.levels {
                    let side = (plan.side >> level).max(1);
                    let mut out = vec![0; side as usize * side as usize * 4];
                    for local in page * per_page..((page + 1) * per_page).min(plan.layers) {
                        let chain = &chains[local as usize];
                        let mip = &chain[chain.len() - plan.levels as usize + level as usize];
                        let tile = plan.tile(level, bytemuck::cast_slice(&mip.pixels))?;
                        let (_, x, y) = plan.placement(local)?;
                        let cell = (plan.cell >> level).max(1);
                        blit(&mut out, side, x >> level, y >> level, cell, cell, &tile);
                    }
                    if level == 0 {
                        texture = Some(device.texture(side, side, plan.levels, &out)?);
                    } else {
                        device.patch(
                            texture.as_ref().ok_or("D3D9 atlas not initialized")?,
                            level,
                            0,
                            0,
                            side,
                            side,
                            &out,
                        )?;
                    }
                }
                pages.push(texture.ok_or("D3D9 atlas has no mip levels")?);
            }
        }
        log::info!(
            "D3D9 sprites: {layers} native-resolution layers -> {} pages ({} animated layers isolated), {} bytes",
            pages.len(),
            animated.len(),
            layout.bytes()
        );
        Ok(Self {
            layout,
            pages,
            mips: settings.to_vec(),
            animations: animations
                .iter()
                .cloned()
                .map(|a| (a, usize::MAX))
                .collect(),
            frame_cache: dx::AnimationCache::new(4 << 20),
            frame_scratch: Vec::new(),
            minimum_size: minimum_size.max(1),
            mipmap_option: None,
        })
    }
    pub fn animate(&mut self, device: &mut dx::Device, ticks: u64) -> Result<()> {
        self.update_animations(device, Some(ticks))
    }
    fn update_animations(&mut self, device: &mut dx::Device, ticks: Option<u64>) -> Result<()> {
        for (animation, previous) in &mut self.animations {
            let frame = match ticks {
                Some(ticks) => animation.frame(ticks),
                None if *previous != usize::MAX => *previous,
                None => continue,
            };
            if ticks.is_some() && *previous == frame {
                continue;
            }
            let (group, local) = self.layout.locations[animation.layer as usize];
            let plan = &self.layout.layouts[group];
            let (page, x, y) = plan.placement(local)?;
            let page = self.layout.offsets[group] + page;
            if !self
                .frame_cache
                .decode(animation.layer, frame, &mut self.frame_scratch)?
            {
                let chain = sprite_mips(
                    animation.pixels(frame),
                    self.mips
                        .get(animation.layer as usize)
                        .copied()
                        .unwrap_or_default(),
                );
                self.frame_scratch.clear();
                for level in 0..plan.levels {
                    let mip = &chain[chain.len() - plan.levels as usize + level as usize];
                    let pixels = plan.tile(level, bytemuck::cast_slice(&mip.pixels))?;
                    self.frame_scratch.extend_from_slice(&pixels);
                }
                self.frame_cache
                    .insert(animation.layer, frame, &self.frame_scratch);
            }
            let mut at = 0;
            for level in 0..self.pages[page].levels {
                let cell = (plan.cell >> level).max(1);
                let len = (cell * cell * 4) as usize;
                device.patch(
                    &self.pages[page],
                    level,
                    x >> level,
                    y >> level,
                    cell,
                    cell,
                    &self.frame_scratch[at..at + len],
                )?;
                at += len;
            }
            *previous = frame;
        }
        Ok(())
    }
    pub fn cpu_bytes(&self) -> usize {
        self.frame_cache.bytes()
            + self.frame_scratch.capacity()
            + self
                .pages
                .iter()
                .map(dx::Texture::mip_cpu_bytes)
                .sum::<usize>()
    }
    pub fn set_mipmap_levels(&mut self, device: &mut dx::Device, requested: u32) -> Result<()> {
        if self.mipmap_option == Some(requested) {
            return Ok(());
        }
        self.mipmap_option = Some(requested);
        let changed = (|| -> Result<()> {
            for (group, plan) in self.layout.layouts.iter().enumerate() {
                let levels = crate::sprites::sprite_mipmap_limit(
                    requested,
                    plan.size,
                    self.minimum_size,
                    plan.levels,
                ) + 1;
                for page in
                    self.layout.offsets[group]..self.layout.offsets[group] + plan.pages as usize
                {
                    device.limit_texture_levels(&mut self.pages[page], levels)?;
                }
            }
            Ok(())
        })();
        // Restore currently displayed frames, including when Animated Textures is Off.
        let refreshed = self.update_animations(device, None);
        changed?;
        refreshed?;
        log::info!(
            "D3D9 mipmap option={requested} native_levels={:?} retained_tail_bytes={}",
            self.pages.iter().map(|p| p.levels).collect::<Vec<_>>(),
            self.pages
                .iter()
                .map(dx::Texture::mip_cpu_bytes)
                .sum::<usize>()
        );
        Ok(())
    }
    pub fn quad(
        &self,
        layer: u32,
        vertices: &[dx::Vertex; 4],
        emit: impl FnMut(usize, &[dx::Vertex]),
    ) -> Result<()> {
        sprite_quad(&self.layout, layer, vertices, emit)
    }
}

pub(super) fn sprite_quad(
    layout: &dx::AtlasSet,
    layer: u32,
    vertices: &[dx::Vertex; 4],
    mut emit: impl FnMut(usize, &[dx::Vertex]),
) -> Result<()> {
    let mapping = layout.transform(layer)?;
    let min = std::array::from_fn::<_, 2, _>(|k| {
        vertices
            .iter()
            .map(|v| v.uv[k])
            .fold(f32::INFINITY, f32::min)
    });
    let max = std::array::from_fn::<_, 2, _>(|k| {
        vertices
            .iter()
            .map(|v| v.uv[k])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let lower = min.map(|v| v.floor() as i32);
    let upper = std::array::from_fn::<_, 2, _>(|k| (max[k].ceil() as i32).max(lower[k] + 1));
    if upper[0] - lower[0] > 32 || upper[1] - lower[1] > 32 {
        return Err("D3D9 sprite quad repeats more than 32 times per axis".into());
    }
    if lower == [0, 0] && upper == [1, 1] {
        let mut out = *vertices;
        for v in &mut out {
            v.uv = mapping.map(v.uv);
        }
        emit(mapping.page, &out);
        return Ok(());
    }
    for y in lower[1]..upper[1] {
        for x in lower[0]..upper[0] {
            let mut polygon = clip_rect(
                vertices.to_vec(),
                [x as f32, y as f32, (x + 1) as f32, (y + 1) as f32],
            );
            if polygon.len() < 3 {
                continue;
            }
            for v in &mut polygon {
                v.uv = mapping.map([v.uv[0] - x as f32, v.uv[1] - y as f32]);
            }
            emit(mapping.page, &polygon);
        }
    }
    Ok(())
}
fn sprite_mips(pixels: &[u8], settings: crate::mipmap::Settings) -> Vec<crate::mipmap::Image> {
    let size = (pixels.len() / 4).isqrt() as u32;
    crate::mipmap::generate(
        crate::mipmap::Image {
            width: size,
            height: size,
            pixels: pixels
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]])
                .collect(),
        },
        size.ilog2(),
        settings,
    )
}

/// Virtual shared-atlas coordinates stay unchanged. Used 128px tiles are packed
/// into managed physical pages in access order, keeping HUD fonts/fills together.
pub(super) struct Pixels {
    pub width: u32,
    pub height: u32,
    image: dx::TileImage,
    main: RefCell<PixelPages>,
    entities: Option<RefCell<PixelPages>>,
    beams: RefCell<beam::Sprites>,
}
struct PixelPages {
    side: u32,
    inverse_side: f32,
    compact: Option<CompactSlots>,
    locations: Vec<Option<(usize, u32, u32)>>,
    pages: Vec<Option<dx::Texture>>,
    slots: u32,
    pending: Vec<usize>,
    dirty: Vec<bool>,
}

struct CompactSlots {
    tiles: Vec<usize>,
    last_seen: Vec<u64>,
    frame: u64,
    uploads_left: u32,
    can_recycle: bool,
    recycled: u64,
    overflow: u64,
}

/// Both layouts use the same virtual texels and clipping rules. Physical UVs
/// belong to the layout they were built for.
#[derive(Clone, Copy)]
pub(super) struct PixelView<'a> {
    image: &'a dx::TileImage,
    layout: &'a RefCell<PixelPages>,
    compact: Option<&'a RefCell<PixelPages>>,
    beams: &'a RefCell<beam::Sprites>,
}

pub(super) struct PixelTextures<'a> {
    main: Ref<'a, [Option<dx::Texture>]>,
    compact: Option<Ref<'a, PixelPages>>,
    beams: Ref<'a, beam::Sprites>,
}

impl PixelTextures<'_> {
    pub fn get(&self, page: usize) -> Option<&dx::Texture> {
        if page == ENTITY_PAGE {
            self.compact.as_ref()?.pages.first()?.as_ref()
        } else if beam::Sprites::uses_page(page) {
            self.beams.get(page)
        } else {
            self.main[page].as_ref()
        }
    }
}

const ENTITY_PAGE_SIDE: u32 = 2048;
pub(super) const ENTITY_PAGE: usize = usize::MAX;
pub(super) const ENTITY_PAGE_BYTES: u64 = ENTITY_PAGE_SIDE as u64 * ENTITY_PAGE_SIDE as u64 * 4;
// Limit compulsory tile uploads while the camera moves into new material runs.
// Existing texture patches still update both layouts.
const ENTITY_TILE_UPLOAD_BYTES: u32 = 512 << 10;

impl CompactSlots {
    fn uploads_per_frame(cell: u32) -> u32 {
        ENTITY_TILE_UPLOAD_BYTES / (cell * cell * 4)
    }

    fn new(image: &dx::TileImage, capacity: usize) -> Result<Self> {
        let mut tiles = Vec::new();
        let mut last_seen = Vec::new();
        tiles.try_reserve_exact(capacity)?;
        last_seen.try_reserve_exact(image.len())?;
        last_seen.resize(image.len(), 0);
        Ok(Self {
            tiles,
            last_seen,
            frame: 0,
            uploads_left: Self::uploads_per_frame(image.cell()),
            can_recycle: true,
            recycled: 0,
            overflow: 0,
        })
    }

    fn begin_frame(&mut self, cell: u32) {
        self.frame += 1;
        self.uploads_left = Self::uploads_per_frame(cell);
        self.can_recycle = true;
    }

    fn oldest_unused(&self) -> Option<(usize, usize)> {
        self.tiles
            .iter()
            .copied()
            .enumerate()
            // Both phases of the current frame protect their coordinates.
            // Keep the previous frame's tiles, too, to avoid cycling through
            // a working set which does not all fit on this page.
            .filter(|&(_, id)| self.frame.saturating_sub(self.last_seen[id]) > 1)
            .min_by_key(|&(_, id)| self.last_seen[id])
    }
}

impl PixelPages {
    fn new(image: &dx::TileImage, side: u32, single_page: bool) -> Result<Self> {
        // Both layouts allocate power-of-two pages, so the reciprocal is exact.
        debug_assert!(side.is_power_of_two());
        let mut locations = Vec::new();
        let mut dirty = Vec::new();
        let mut pending = Vec::new();
        let mut pages = Vec::new();
        locations.try_reserve_exact(image.len())?;
        dirty.try_reserve_exact(image.len())?;
        locations.resize(image.len(), None);
        dirty.resize(image.len(), false);
        let compact = if single_page {
            let capacity = ((side / image.cell()).pow(2) as usize).min(image.len());
            pending.try_reserve_exact(capacity)?;
            pages.try_reserve_exact(1)?;
            Some(CompactSlots::new(image, capacity)?)
        } else {
            None
        };
        Ok(Self {
            side,
            inverse_side: (side as f32).recip(),
            compact,
            locations,
            pages,
            slots: 0,
            pending,
            dirty,
        })
    }

    fn queue(&mut self, id: usize) {
        if self.locations[id].is_some() && !self.dirty[id] {
            self.pending.push(id);
            self.dirty[id] = true;
        }
    }

    fn place(&mut self, image: &dx::TileImage, id: usize) -> Option<(usize, u32, u32)> {
        if let Some(location) = self.locations[id] {
            if let Some(compact) = &mut self.compact {
                compact.last_seen[id] = compact.frame;
            }
            return Some(location);
        }
        let mut location = image.placement(self.slots, self.side)?;
        let append = self.compact.is_none() || location.0 == 0;
        if let Some(compact) = &mut self.compact {
            if compact.uploads_left == 0 || (!append && !compact.can_recycle) {
                compact.overflow += 1;
                return None;
            }
            if append {
                compact.tiles.push(id);
            } else if let Some((slot, old)) = compact.oldest_unused() {
                location = self.locations[old].take().unwrap();
                if std::mem::take(&mut self.dirty[old]) {
                    self.pending.retain(|&queued| queued != old);
                }
                compact.tiles[slot] = id;
                compact.recycled += 1;
            } else {
                // Eligibility can only decrease again within this frame.
                compact.can_recycle = false;
                compact.overflow += 1;
                return None;
            }
            compact.uploads_left -= 1;
            compact.last_seen[id] = compact.frame;
        }
        self.slots += u32::from(append);
        if location.0 == self.pages.len() {
            self.pages.push(None);
        }
        self.locations[id] = Some(location);
        self.queue(id);
        Some(location)
    }

    fn bytes(&self) -> u64 {
        self.pages.iter().flatten().map(|p| p.bytes).sum()
    }

    fn cpu_bytes(&self) -> usize {
        self.locations.capacity() * size_of::<Option<(usize, u32, u32)>>()
            + self.pending.capacity() * size_of::<usize>()
            + self.dirty.capacity()
            + self.pages.capacity() * size_of::<Option<dx::Texture>>()
            + self.compact.as_ref().map_or(0, |compact| {
                compact.tiles.capacity() * size_of::<usize>()
                    + compact.last_seen.capacity() * size_of::<u64>()
            })
    }
}

impl Pixels {
    pub fn new(
        device: &mut dx::Device,
        pixels: &[u8],
        width: u32,
        height: u32,
        page_budget: u64,
    ) -> Result<Self> {
        let tile = width.max(height).min(128).next_power_of_two();
        let image = dx::TileImage::new(pixels, width, height, tile, 1)?;
        let mut side =
            device
                .caps
                .max_texture
                .min(if page_budget >= 64 << 20 { 2048 } else { 1024 });
        while u64::from(side).pow(2) * 4 > page_budget {
            side /= 2;
        }
        side = 1u32 << side.max(1).ilog2();
        // Standalone small pictures must not reserve an entire HUD-sized page.
        while side / 2 >= image.cell()
            && u64::from(side / 2 / image.cell()).pow(2) >= image.len() as u64
        {
            side /= 2;
        }
        if side < image.cell() {
            return Err("D3D9 caps/budget cannot hold a GUI tile".into());
        }
        let main = RefCell::new(PixelPages::new(&image, side, false)?);
        Ok(Self {
            width,
            height,
            image,
            main,
            entities: None,
            beams: Default::default(),
        })
    }

    pub fn view(&self) -> PixelView<'_> {
        PixelView {
            image: &self.image,
            layout: &self.main,
            compact: None,
            beams: &self.beams,
        }
    }

    pub fn entity_view(&self) -> Option<PixelView<'_>> {
        Some(PixelView {
            image: &self.image,
            layout: &self.main,
            compact: Some(self.entities.as_ref()?),
            beams: &self.beams,
        })
    }

    pub fn entity_or_main(&self, compact: bool) -> PixelView<'_> {
        if compact && let Some(view) = self.entity_view() {
            return view;
        }
        self.view()
    }

    pub fn supports_entity_page(&self, device: &dx::Device) -> bool {
        self.main.borrow().side < ENTITY_PAGE_SIDE && device.caps.max_texture >= ENTITY_PAGE_SIDE
    }

    pub fn enable_entity_page(&mut self) -> Result<()> {
        self.entities = Some(RefCell::new(PixelPages::new(
            &self.image,
            ENTITY_PAGE_SIDE,
            true,
        )?));
        Ok(())
    }

    pub fn drop_entity_page(&mut self) {
        self.entities = None;
    }

    pub fn begin_entity_frame(&mut self) {
        if let Some(page) = &mut self.entities {
            page.get_mut()
                .compact
                .as_mut()
                .unwrap()
                .begin_frame(self.image.cell());
        }
    }

    pub fn entity_page_stats(&self) -> (u32, u64, u64) {
        self.entities.as_ref().map_or((0, 0, 0), |page| {
            let page = page.borrow();
            let compact = page.compact.as_ref().unwrap();
            (page.slots, compact.recycled, compact.overflow)
        })
    }

    pub fn bytes(&self) -> u64 {
        self.main.borrow().bytes()
            + self.beams.borrow().bytes()
            + self
                .entities
                .as_ref()
                .map_or(0, |pages| pages.borrow().bytes())
    }

    pub fn cpu_bytes(&self) -> usize {
        self.image.bytes()
            + self.main.borrow().cpu_bytes()
            + self
                .entities
                .as_ref()
                .map_or(0, |pages| pages.borrow().cpu_bytes())
    }

    pub fn textures(&self) -> std::cell::Ref<'_, [Option<dx::Texture>]> {
        self.view().textures()
    }

    pub fn prepare(&self, device: &mut dx::Device) -> Result<()> {
        self.view().prepare(device)
    }

    pub fn patch(&mut self, x: u32, y: u32, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        for id in self.image.patch(x, y, width, height, rgba)? {
            self.main.get_mut().queue(id);
            if let Some(pages) = &mut self.entities {
                pages.get_mut().queue(id);
            }
        }
        self.beams.get_mut().dirty([x, y, width, height]);
        Ok(())
    }

    pub fn revision(&self) -> u64 {
        self.image.presence_revision()
    }

    pub fn polygon(&self, input: &[dx::Vertex], emit: impl FnMut(usize, &[dx::Vertex])) {
        self.view().polygon(input, emit);
    }
}

impl<'a> PixelView<'a> {
    pub fn textures(self) -> Ref<'a, [Option<dx::Texture>]> {
        Ref::map(self.layout.borrow(), |s| s.pages.as_slice())
    }

    pub fn draw_textures(self) -> PixelTextures<'a> {
        PixelTextures {
            main: self.textures(),
            compact: self.compact.map(RefCell::borrow),
            beams: self.beams.borrow(),
        }
    }

    pub fn repeat_v_texture(
        self,
        device: &mut dx::Device,
        rect: [f32; 4],
    ) -> Result<Option<(usize, u32)>> {
        self.beams.borrow_mut().prepare(self.image, device, rect)
    }

    pub fn prepare(self, device: &mut dx::Device) -> Result<()> {
        self.prepare_layout(device, self.layout)
    }

    pub fn prepare_compact(self, device: &mut dx::Device) -> Result<()> {
        if let Some(page) = self.compact {
            self.prepare_layout(device, page)?;
        }
        Ok(())
    }

    fn prepare_layout(self, device: &mut dx::Device, layout: &RefCell<PixelPages>) -> Result<()> {
        let mut s = layout.borrow_mut();
        if s.pending.is_empty() {
            return Ok(());
        }
        let mut scratch = Vec::new();
        let side = s.side;
        // Remove only after all writes succeed; device-loss recovery may retry.
        for i in 0..s.pending.len() {
            let id = s.pending[i];
            let (page, x, y) = s.locations[id].unwrap();
            if s.pages[page].is_none() {
                let texture = if s.compact.is_some() {
                    let _init = crate::spans::guard("d3d9_entity_page_init");
                    device.zeroed_texture(side, side)?
                } else {
                    let bytes = (side * side * 4) as usize;
                    let mut blank = Vec::new();
                    blank.try_reserve_exact(bytes)?;
                    blank.resize(bytes, 0);
                    device.texture(side, side, 1, &blank)?
                };
                s.pages[page] = Some(texture);
            }
            self.image.decode(id, &mut scratch)?;
            let cell = self.image.cell();
            device.patch(
                s.pages[page].as_ref().unwrap(),
                0,
                x,
                y,
                cell,
                cell,
                &scratch,
            )?;
        }
        while let Some(id) = s.pending.pop() {
            s.dirty[id] = false;
        }
        Ok(())
    }
    /// Route every tile through its assigned layout in polygon order. A miss
    /// on the compact page can always use the corresponding main-atlas tile.
    pub fn polygon(self, input: &[dx::Vertex], mut emit: impl FnMut(usize, &[dx::Vertex])) {
        if input.is_empty() {
            return;
        }
        let mut main = self.layout.borrow_mut();
        let mut compact = self.compact.map(RefCell::borrow_mut);
        let image = self.image;
        let tile = image.tile;
        debug_assert!(tile.is_power_of_two());
        let tile_shift = tile.trailing_zeros();
        let min = std::array::from_fn::<_, 2, _>(|k| {
            input
                .iter()
                .map(|v| v.uv[k])
                .fold(f32::INFINITY, f32::min)
                .clamp(0.0, 1.0)
        });
        let max = std::array::from_fn::<_, 2, _>(|k| {
            input
                .iter()
                .map(|v| v.uv[k])
                .fold(f32::NEG_INFINITY, f32::max)
                .clamp(0.0, 1.0)
        });
        let axis = [image.width, image.height];
        let tile_counts = [image.columns, image.rows];
        let tile_at = |texel: u32, axis_index: usize| {
            if dx::development_build() {
                (texel >> tile_shift).min(tile_counts[axis_index] - 1)
            } else {
                (texel / tile).min(axis[axis_index].div_ceil(tile) - 1)
            }
        };
        let start =
            std::array::from_fn::<_, 2, _>(|k| tile_at((min[k] * axis[k] as f32) as u32, k));
        let end = std::array::from_fn::<_, 2, _>(|k| {
            tile_at((max[k] * axis[k] as f32).ceil().max(1.0) as u32 - 1, k).max(start[k])
        });
        for y in start[1]..=end[1] {
            for x in start[0]..=end[0] {
                let id = (y * image.columns + x) as usize;
                if !image.present(id) {
                    continue;
                }
                let (page, ox, oy, side, inverse_side) = compact
                    .as_mut()
                    .and_then(|s| {
                        s.place(image, id)
                            .map(|(_, x, y)| (ENTITY_PAGE, x, y, s.side, s.inverse_side))
                    })
                    .unwrap_or_else(|| {
                        let (page, x, y) = main.place(image, id).unwrap();
                        (page, x, y, main.side, main.inverse_side)
                    });
                let mut polygon: smallvec::SmallVec<[dx::Vertex; 8]> = if start == end {
                    smallvec::SmallVec::from_slice(input)
                } else {
                    clip_rect(
                        input.to_vec(),
                        [
                            x as f32 * tile as f32 / image.width as f32,
                            y as f32 * tile as f32 / image.height as f32,
                            ((x + 1) * tile) as f32 / image.width as f32,
                            ((y + 1) * tile) as f32 / image.height as f32,
                        ],
                    )
                    .into_iter()
                    .collect()
                };
                if polygon.len() < 3 {
                    continue;
                }
                let normalize = |texel: f32| {
                    if dx::development_build() {
                        texel * inverse_side
                    } else {
                        texel / side as f32
                    }
                };
                for v in &mut polygon {
                    v.uv = [
                        normalize(
                            v.uv[0] * image.width as f32 - x as f32 * tile as f32
                                + ox as f32
                                + image.padding as f32,
                        ),
                        normalize(
                            v.uv[1] * image.height as f32 - y as f32 * tile as f32
                                + oy as f32
                                + image.padding as f32,
                        ),
                    ];
                }
                emit(page, &polygon);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_uvs_match_division_for_fractional_texel_positions() {
        let image = dx::TileImage::new(&[255; 17 * 13 * 4], 17, 13, 8, 1).unwrap();
        let pixels = Pixels {
            width: 17,
            height: 13,
            main: RefCell::new(PixelPages::new(&image, 16, false).unwrap()),
            entities: Some(RefCell::new(PixelPages::new(&image, 32, true).unwrap())),
            beams: Default::default(),
            image,
        };
        let quad = |left, right| {
            [[left, 0.13], [right, 0.13], [right, 0.52], [left, 0.52]].map(|uv| dx::Vertex {
                pos: [0.0; 3],
                uv,
                color: 0xffffffff,
                light: [0.5; 2],
            })
        };
        for (view, side, pages, offset) in [
            (pixels.view(), 16.0, [0, 1], 0.0),
            (pixels.entity_view().unwrap(), 32.0, [ENTITY_PAGE; 2], 10.0),
        ] {
            let mut draws = 0;
            view.polygon(&quad(0.14, 8.0 / 17.0), |page, vertices| {
                assert_eq!(page, pages[0]);
                assert_eq!(vertices.len(), 4);
                draws += 1;
            });
            assert_eq!(draws, 1);
            let input = quad(0.52, 0.84);
            let mut draws = 0;
            view.polygon(&input, |page, vertices| {
                assert_eq!(page, pages[1]);
                assert_eq!(vertices.len(), input.len());
                for (vertex, source) in vertices.iter().zip(input) {
                    let old_u = (source.uv[0] * 17.0 - 8.0 + offset + 1.0) / side;
                    let old_v = (source.uv[1] * 13.0 + 1.0) / side;
                    assert_eq!(
                        vertex.uv.map(f32::to_bits),
                        [old_u, old_v].map(f32::to_bits)
                    );
                }
                draws += 1;
            });
            assert_eq!(draws, 1);
        }
    }

    #[test]
    fn compact_page_keeps_seam_texels_and_falls_back_when_full() {
        // Nine padded 8px tiles fit in the compact page and one in each
        // ordinary page. The quad covers the seam between the first two.
        let image = dx::TileImage::new(&[255; 128 * 8 * 4], 128, 8, 8, 1).unwrap();
        let mut pixels = Pixels {
            width: 128,
            height: 8,
            main: RefCell::new(PixelPages::new(&image, 16, false).unwrap()),
            entities: Some(RefCell::new(PixelPages::new(&image, 32, true).unwrap())),
            beams: Default::default(),
            image,
        };
        let quad = [[6.0, 2.0], [10.0, 2.0], [10.0, 6.0], [6.0, 6.0]].map(|[x, y]| dx::Vertex {
            pos: [x, y, 0.0],
            uv: [x / 128.0, y / 8.0],
            color: 0xffffffff,
            light: [0.5; 2],
        });
        for (view, side, pages, offsets) in [
            (pixels.view(), 16.0, [0, 1], [0.0, 0.0]),
            (
                pixels.entity_view().unwrap(),
                32.0,
                [ENTITY_PAGE; 2],
                [0.0, 10.0],
            ),
        ] {
            let mut at = 0;
            view.polygon(&quad, |page, poly| {
                assert_eq!(page, pages[at]);
                assert_eq!(poly.len(), 4);
                for v in poly {
                    assert_eq!(
                        v.uv[0] * side - offsets[at],
                        v.pos[0] - (8 * at) as f32 + 1.0
                    );
                    assert_eq!(v.uv[1] * side, v.pos[1] + 1.0);
                    assert_eq!(v.light, [0.5; 2]);
                    assert_eq!(v.color, 0xffffffff);
                }
                at += 1;
            });
            assert_eq!(at, 2);
        }
        // Simulate completion of those uploads, then change texels across the
        // gutter boundary. Both layouts must queue both existing tiles.
        for layout in [&mut pixels.main, pixels.entities.as_mut().unwrap()] {
            let layout = layout.get_mut();
            layout.pending.clear();
            layout.dirty.fill(false);
        }
        pixels.patch(7, 3, 2, 1, &[23; 8]).unwrap();
        assert_eq!(pixels.main.borrow().pending, [0, 1]);
        assert_eq!(pixels.entities.as_ref().unwrap().borrow().pending, [0, 1]);

        let mut full = quad;
        full[0].uv[0] = 0.0;
        full[3].uv[0] = 0.0;
        full[1].uv[0] = 80.0 / 128.0;
        full[2].uv[0] = 80.0 / 128.0;
        let mut pages = Vec::new();
        pixels
            .entity_view()
            .unwrap()
            .polygon(&full, |page, _| pages.push(page));
        assert_eq!(&pages[..9], &[ENTITY_PAGE; 9]);
        assert_eq!(&pages[9..], &[2]);
        assert_eq!(pixels.entity_page_stats(), (9, 0, 1));
    }

    #[test]
    fn compact_slots_recycle_only_old_tiles_within_the_frame_budget() {
        let image = dx::TileImage::new(&[255; 128 * 8 * 4], 128, 8, 8, 1).unwrap();
        let mut page = PixelPages::new(&image, 32, true).unwrap();
        for id in 0..9 {
            assert!(page.place(&image, id).is_some());
        }
        let replace_at = page.locations[2];
        page.compact.as_mut().unwrap().begin_frame(image.cell());
        assert!(page.place(&image, 9).is_none());
        assert!(page.place(&image, 1).is_some());

        let compact = page.compact.as_mut().unwrap();
        compact.begin_frame(image.cell());
        compact.uploads_left = 1;
        assert!(page.place(&image, 0).is_some());
        assert_eq!(page.place(&image, 9), replace_at);
        assert!(page.place(&image, 10).is_none());
        assert_eq!(page.slots, 9);
        assert_eq!(page.locations[2], None);
        assert!(!page.dirty[2]);
        assert!(!page.pending.contains(&2));
        assert!(page.pending.contains(&9));
        let compact = page.compact.unwrap();
        assert_eq!((compact.recycled, compact.overflow), (1, 2));
    }
}

fn blit(dest: &mut [u8], stride: u32, x: u32, y: u32, width: u32, height: u32, src: &[u8]) {
    for row in 0..height {
        let start = ((y + row) * stride + x) as usize * 4;
        dest[start..start + width as usize * 4].copy_from_slice(
            &src[row as usize * width as usize * 4..(row + 1) as usize * width as usize * 4],
        );
    }
}
pub(super) fn clip_rect(mut poly: Vec<dx::Vertex>, rect: [f32; 4]) -> Vec<dx::Vertex> {
    for (axis, limit, below) in [
        (0, rect[0], false),
        (0, rect[2], true),
        (1, rect[1], false),
        (1, rect[3], true),
    ] {
        let inside = |v: &dx::Vertex| {
            if below {
                v.uv[axis] <= limit
            } else {
                v.uv[axis] >= limit
            }
        };
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            let ia = inside(&a);
            let ib = inside(&b);
            if ia {
                out.push(a);
            }
            if ia != ib {
                let t = (limit - a.uv[axis]) / (b.uv[axis] - a.uv[axis]);
                let lerp = |x: f32, y: f32| x + (y - x) * t;
                let color = (0..4)
                    .map(|c| {
                        let shift = c * 8;
                        let x = (a.color >> shift & 255) as f32;
                        let y = (b.color >> shift & 255) as f32;
                        (lerp(x, y).clamp(0.0, 255.0) as u32) << shift
                    })
                    .fold(0, |a, b| a | b);
                out.push(dx::Vertex {
                    pos: std::array::from_fn(|i| lerp(a.pos[i], b.pos[i])),
                    uv: std::array::from_fn(|i| lerp(a.uv[i], b.uv[i])),
                    light: std::array::from_fn(|i| lerp(a.light[i], b.light[i])),
                    color,
                });
            }
        }
        poly = out;
    }
    poly
}
pub(super) fn quads(poly: &[dx::Vertex], out: &mut Vec<dx::Vertex>) {
    if poly.len() == 4 {
        out.extend_from_slice(poly);
    } else {
        for i in 1..poly.len().saturating_sub(1) {
            out.extend_from_slice(&[poly[0], poly[i], poly[i + 1], poly[i + 1]]);
        }
    }
}
pub(super) fn triangles(poly: &[dx::Vertex], out: &mut Vec<dx::Vertex>) {
    for i in 1..poly.len().saturating_sub(1) {
        out.extend_from_slice(&[poly[0], poly[i], poly[i + 1]]);
    }
}
