use super::*;

pub(super) struct Sprites {
    pub layout: Arc<dx::AtlasSet>,
    pub pages: Vec<dx::Texture>,
    pub animations: Vec<(SpriteAnimation, usize)>,
    mips: Vec<crate::mipmap::Settings>,
    pub frame_cache: dx::AnimationCache,
    frame_scratch: Vec<u8>,
}
impl Sprites {
    pub fn new(
        device: &mut dx::Device,
        pixels: &[u8],
        size: u32,
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
        })
    }
    pub fn animate(&mut self, device: &mut dx::Device, ticks: u64) -> Result<()> {
        for (animation, previous) in &mut self.animations {
            let frame = animation.frame(ticks);
            if *previous == frame {
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
            for level in 0..plan.levels {
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
        self.frame_cache.bytes() + self.frame_scratch.capacity()
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
    side: u32,
    state: std::cell::RefCell<PixelCache>,
}
struct PixelCache {
    image: dx::TileImage,
    locations: Vec<Option<(usize, u32, u32)>>,
    pages: Vec<Option<dx::Texture>>,
    slots: u32,
    pending: Vec<usize>,
    dirty: Vec<bool>,
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
        let len = image.len();
        Ok(Self {
            width,
            height,
            side,
            state: std::cell::RefCell::new(PixelCache {
                image,
                locations: vec![None; len],
                pages: Vec::new(),
                slots: 0,
                pending: Vec::new(),
                dirty: vec![false; len],
            }),
        })
    }
    pub fn bytes(&self) -> u64 {
        self.state
            .borrow()
            .pages
            .iter()
            .flatten()
            .map(|p| p.bytes)
            .sum()
    }
    pub fn cpu_bytes(&self) -> usize {
        let s = self.state.borrow();
        s.image.bytes()
            + s.locations.capacity() * size_of::<Option<(usize, u32, u32)>>()
            + s.pending.capacity() * size_of::<usize>()
            + s.dirty.capacity()
    }
    pub fn textures(&self) -> std::cell::Ref<'_, [Option<dx::Texture>]> {
        std::cell::Ref::map(self.state.borrow(), |s| s.pages.as_slice())
    }
    pub fn prepare(&self, device: &mut dx::Device) -> Result<()> {
        let mut s = self.state.borrow_mut();
        if s.pending.is_empty() {
            return Ok(());
        }
        let mut scratch = Vec::new();
        // Remove only after all writes succeed; device-loss recovery may retry.
        for i in 0..s.pending.len() {
            let id = s.pending[i];
            let (page, x, y) = s.locations[id].unwrap();
            if s.pages[page].is_none() {
                s.pages[page] = Some(device.texture(
                    self.side,
                    self.side,
                    1,
                    &vec![0; (self.side * self.side * 4) as usize],
                )?);
            }
            s.image.decode(id, &mut scratch)?;
            let cell = s.image.cell();
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
    pub fn patch(
        &mut self,
        _device: &mut dx::Device,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        let s = self.state.get_mut();
        for id in s.image.patch(x, y, width, height, rgba)? {
            if s.locations[id].is_some() && !s.dirty[id] {
                s.dirty[id] = true;
                s.pending.push(id);
            }
        }
        Ok(())
    }
    pub fn revision(&self) -> u64 {
        self.state.borrow().image.presence_revision()
    }
    pub fn polygon(&self, input: &[dx::Vertex], mut emit: impl FnMut(usize, &[dx::Vertex])) {
        if input.is_empty() {
            return;
        }
        let mut s = self.state.borrow_mut();
        let tile = s.image.tile;
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
        let axis = [self.width, self.height];
        let start = std::array::from_fn::<_, 2, _>(|k| {
            ((min[k] * axis[k] as f32) as u32 / tile).min(axis[k].div_ceil(tile) - 1)
        });
        let end = std::array::from_fn::<_, 2, _>(|k| {
            (((max[k] * axis[k] as f32).ceil().max(1.0) as u32 - 1) / tile)
                .min(axis[k].div_ceil(tile) - 1)
                .max(start[k])
        });
        for y in start[1]..=end[1] {
            for x in start[0]..=end[0] {
                let id = (y * s.image.columns + x) as usize;
                if !s.image.present(id) {
                    continue;
                }
                let (page, ox, oy) = match s.locations[id] {
                    Some(location) => location,
                    None => {
                        let slot = s.slots;
                        s.slots += 1;
                        let location = s.image.placement(slot, self.side).unwrap();
                        if location.0 == s.pages.len() {
                            s.pages.push(None);
                        }
                        s.locations[id] = Some(location);
                        s.pending.push(id);
                        s.dirty[id] = true;
                        location
                    }
                };
                let mut polygon: smallvec::SmallVec<[dx::Vertex; 8]> = if start == end {
                    smallvec::SmallVec::from_slice(input)
                } else {
                    clip_rect(
                        input.to_vec(),
                        [
                            x as f32 * tile as f32 / self.width as f32,
                            y as f32 * tile as f32 / self.height as f32,
                            ((x + 1) * tile) as f32 / self.width as f32,
                            ((y + 1) * tile) as f32 / self.height as f32,
                        ],
                    )
                    .into_iter()
                    .collect()
                };
                if polygon.len() < 3 {
                    continue;
                }
                for v in &mut polygon {
                    v.uv = [
                        (v.uv[0] * self.width as f32 - x as f32 * tile as f32
                            + ox as f32
                            + s.image.padding as f32)
                            / self.side as f32,
                        (v.uv[1] * self.height as f32 - y as f32 * tile as f32
                            + oy as f32
                            + s.image.padding as f32)
                            / self.side as f32,
                    ];
                }
                emit(page, &polygon);
            }
        }
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
