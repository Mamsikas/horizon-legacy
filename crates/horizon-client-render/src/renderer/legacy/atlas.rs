use super::*;

pub(super) struct Sprites {
    pub layout: dx::AtlasSet,
    pub pages: Vec<dx::Texture>,
    pub animations: Vec<(SpriteAnimation, usize)>,
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
        let layout = dx::AtlasSet::new(&sizes, &animated, device.caps.max_texture)?;
        let mut pages = Vec::new();
        for (group, plan) in layout.layouts.iter().enumerate() {
            let mut sources = vec![0; plan.layers as usize];
            for (layer, &(g, local)) in layout.locations.iter().enumerate() {
                if g == group {
                    sources[local as usize] = layer;
                }
            }
            let per_page = plan.columns * plan.columns;
            for page in 0..plan.pages {
                let mut texture = None;
                for level in 0..plan.levels {
                    let side = (plan.side >> level).max(1);
                    let mut out = vec![0; side as usize * side as usize * 4];
                    for local in page * per_page..((page + 1) * per_page).min(plan.layers) {
                        let start = sources[local as usize] * size as usize * size as usize * 4;
                        let tile = plan.tile(
                            level,
                            &pixels[start..start + size as usize * size as usize * 4],
                        )?;
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
            animations: animations
                .iter()
                .cloned()
                .map(|a| (a, usize::MAX))
                .collect(),
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
            for level in 0..plan.levels {
                let cell = (plan.cell >> level).max(1);
                let pixels = plan.tile(level, animation.pixels(frame))?;
                device.patch(
                    &self.pages[page],
                    level,
                    x >> level,
                    y >> level,
                    cell,
                    cell,
                    &pixels,
                )?;
            }
            *previous = frame;
        }
        Ok(())
    }
    pub fn quad(
        &self,
        layer: u32,
        vertices: &[dx::Vertex; 4],
        mut emit: impl FnMut(usize, &[dx::Vertex]),
    ) -> Result<()> {
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
            let mut page = 0;
            for v in &mut out {
                let (p, uv) = self.layout.uv(layer, v.uv)?;
                page = p;
                v.uv = uv;
            }
            emit(page, &out);
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
                let mut page = 0;
                for v in &mut polygon {
                    let (p, uv) = self
                        .layout
                        .uv(layer, [v.uv[0] - x as f32, v.uv[1] - y as f32])?;
                    page = p;
                    v.uv = uv;
                }
                emit(page, &polygon);
            }
        }
        Ok(())
    }
}

/// The existing GUI atlas may exceed the device's dimensions. Keep its coordinates
/// and clip polygons at page boundaries; allocate entirely transparent pages lazily.
pub(super) struct Pixels {
    pub width: u32,
    pub height: u32,
    pub page_width: u32,
    pub page_height: u32,
    pub columns: u32,
    pub pages: Vec<Option<dx::Texture>>,
}
impl Pixels {
    pub fn new(
        device: &mut dx::Device,
        pixels: &[u8],
        width: u32,
        height: u32,
        page_budget: u64,
    ) -> Result<Self> {
        if width == 0 || height == 0 || pixels.len() != width as usize * height as usize * 4 {
            return Err("D3D9 GUI image dimensions mismatch".into());
        }
        let mut cap = device.caps.max_texture.min(512);
        while u64::from(cap).pow(2) * 4 > page_budget {
            cap /= 2;
        }
        let cap = 1u32 << cap.max(1).ilog2();
        let page_width = width.next_power_of_two().min(cap);
        let page_height = height.next_power_of_two().min(cap);
        let columns = width.div_ceil(page_width);
        let rows = height.div_ceil(page_height);
        let mut result = Self {
            width,
            height,
            page_width,
            page_height,
            columns,
            pages: (0..columns * rows).map(|_| None).collect(),
        };
        result.patch(device, 0, 0, width, height, pixels)?;
        Ok(result)
    }
    pub fn bytes(&self) -> u64 {
        self.pages.iter().flatten().map(|p| p.bytes).sum()
    }
    pub fn patch(
        &mut self,
        device: &mut dx::Device,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        if x.checked_add(width).is_none_or(|v| v > self.width)
            || y.checked_add(height).is_none_or(|v| v > self.height)
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err("D3D9 GUI patch outside atlas".into());
        }
        for py in y / self.page_height..=(y + height - 1) / self.page_height {
            for px in x / self.page_width..=(x + width - 1) / self.page_width {
                let ox = px * self.page_width;
                let oy = py * self.page_height;
                let x0 = x.max(ox);
                let y0 = y.max(oy);
                let x1 = (x + width).min(ox + self.page_width);
                let y1 = (y + height).min(oy + self.page_height);
                let mut bytes = Vec::with_capacity((x1 - x0) as usize * (y1 - y0) as usize * 4);
                for row in y0..y1 {
                    let start = ((row - y) * width + x0 - x) as usize * 4;
                    bytes.extend_from_slice(&rgba[start..start + (x1 - x0) as usize * 4]);
                }
                let page = &mut self.pages[(py * self.columns + px) as usize];
                if page.is_none() {
                    if !bytes.chunks_exact(4).any(|p| p[3] != 0) {
                        continue;
                    }
                    *page = Some(device.texture(
                        self.page_width,
                        self.page_height,
                        1,
                        &vec![0; self.page_width as usize * self.page_height as usize * 4],
                    )?);
                }
                device.patch(
                    page.as_ref().ok_or("D3D9 GUI page missing")?,
                    0,
                    x0 - ox,
                    y0 - oy,
                    x1 - x0,
                    y1 - y0,
                    &bytes,
                )?;
            }
        }
        Ok(())
    }
    pub fn polygon(&self, input: &[dx::Vertex], mut emit: impl FnMut(usize, &[dx::Vertex])) {
        if input.is_empty() {
            return;
        }
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
        let tile = [self.page_width, self.page_height];
        let start = std::array::from_fn::<_, 2, _>(|k| {
            ((min[k] * axis[k] as f32) as u32 / tile[k]).min(axis[k].div_ceil(tile[k]) - 1)
        });
        let end = std::array::from_fn::<_, 2, _>(|k| {
            (((max[k] * axis[k] as f32).ceil().max(1.0) as u32 - 1) / tile[k])
                .min(axis[k].div_ceil(tile[k]) - 1)
                .max(start[k])
        });
        if start == end && input.len() <= 8 {
            let page = (start[1] * self.columns + start[0]) as usize;
            if self.pages[page].is_some() {
                let mut polygon = smallvec::SmallVec::<[dx::Vertex; 8]>::from_slice(input);
                for v in &mut polygon {
                    v.uv = [
                        (v.uv[0] * self.width as f32 - start[0] as f32 * self.page_width as f32)
                            / self.page_width as f32,
                        (v.uv[1] * self.height as f32 - start[1] as f32 * self.page_height as f32)
                            / self.page_height as f32,
                    ];
                }
                emit(page, &polygon);
            }
            return;
        }
        for y in start[1]..=end[1] {
            for x in start[0]..=end[0] {
                let page = (y * self.columns + x) as usize;
                if self.pages[page].is_none() {
                    continue;
                }
                let rect = [
                    x as f32 * self.page_width as f32 / self.width as f32,
                    y as f32 * self.page_height as f32 / self.height as f32,
                    ((x + 1) * self.page_width) as f32 / self.width as f32,
                    ((y + 1) * self.page_height) as f32 / self.height as f32,
                ];
                let mut polygon = if start == end {
                    input.to_vec()
                } else {
                    clip_rect(input.to_vec(), rect)
                };
                if polygon.len() < 3 {
                    continue;
                }
                for v in &mut polygon {
                    v.uv = [
                        (v.uv[0] * self.width as f32 - x as f32 * self.page_width as f32)
                            / self.page_width as f32,
                        (v.uv[1] * self.height as f32 - y as f32 * self.page_height as f32)
                            / self.page_height as f32,
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
