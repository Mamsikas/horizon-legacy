//! Power-of-two Texture2D pages. Every sprite has its own extruded mip chain;
//! no mip ever averages pixels belonging to neighbouring sprites.
#[derive(Clone, Copy, Debug)]
pub struct AtlasLayout {
    pub size: u32,
    pub cell: u32,
    pub padding: u32,
    pub side: u32,
    pub columns: u32,
    pub layers: u32,
    pub pages: u32,
    pub levels: u32,
}
impl AtlasLayout {
    pub fn new(size: u32, layers: u32, max_texture: u32, page_budget: u64) -> Result<Self, String> {
        if size == 0 || layers == 0 || !size.is_power_of_two() {
            return Err("D3D9 sprites must be nonempty power-of-two squares".into());
        }
        let padding = (size / 2).max(1);
        let cell = size
            .checked_add(2 * padding)
            .and_then(u32::checked_next_power_of_two)
            .ok_or("D3D9 sprite size overflow")?;
        let mut limit = max_texture;
        while u64::from(limit) * u64::from(limit) * 4 > page_budget {
            limit /= 2;
        }
        if limit < cell {
            return Err(format!(
                "D3D9 sprite {size} with mip gutters requires {cell}x{cell}, caps/budget allow {limit}"
            ));
        }
        let mut side = cell;
        while side <= limit / 2 && (side / cell).pow(2) < layers {
            side *= 2;
        }
        let columns = side / cell;
        let per_page = columns * columns;
        Ok(Self {
            size,
            cell,
            padding,
            side,
            columns,
            layers,
            pages: layers.div_ceil(per_page),
            levels: size.ilog2() + 1,
        })
    }
    pub fn placement(&self, layer: u32) -> Result<(usize, u32, u32), String> {
        if layer >= self.layers {
            return Err(format!("D3D9 sprite layer {layer} exceeds {}", self.layers));
        }
        let per_page = self.columns * self.columns;
        let slot = layer % per_page;
        Ok((
            (layer / per_page) as usize,
            slot % self.columns * self.cell,
            slot / self.columns * self.cell,
        ))
    }
    pub fn uv(&self, layer: u32, uv: [f32; 2]) -> Result<(usize, [f32; 2]), String> {
        let (page, x, y) = self.placement(layer)?;
        Ok((
            page,
            [
                (x as f32 + self.padding as f32 + uv[0] * self.size as f32) / self.side as f32,
                (y as f32 + self.padding as f32 + uv[1] * self.size as f32) / self.side as f32,
            ],
        ))
    }
    /// A whole cell, including edge extrusion, for a source image of any square resolution.
    pub fn tile(&self, level: u32, source: &[u8]) -> Result<Vec<u8>, String> {
        let source_size = (source.len() / 4).isqrt();
        if source_size == 0 || source_size * source_size * 4 != source.len() || level >= self.levels
        {
            return Err("D3D9 invalid sprite image/mip level".into());
        }
        let cell = (self.cell >> level).max(1) as usize;
        let inset = (self.padding >> level) as usize;
        let size = (self.size >> level).max(1) as usize;
        let mut texels = vec![[0u8; 4]; size * size];
        for y in 0..size {
            for x in 0..size {
                let x0 = x * source_size / size;
                let y0 = y * source_size / size;
                let x1 = ((x + 1) * source_size / size).max(x0 + 1).min(source_size);
                let y1 = ((y + 1) * source_size / size).max(y0 + 1).min(source_size);
                let mut sum = [0u64; 4];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let pixel = &source[(sy * source_size + sx) * 4..][..4];
                        for c in 0..3 {
                            sum[c] += u64::from(pixel[c]) * u64::from(pixel[3]);
                        }
                        sum[3] += u64::from(pixel[3]);
                    }
                }
                let mut out = [0; 4];
                if sum[3] != 0 {
                    for c in 0..3 {
                        out[c] = (sum[c] / sum[3]) as u8;
                    }
                }
                out[3] = (sum[3] / ((x1 - x0) * (y1 - y0)) as u64) as u8;
                texels[y * size + x] = out;
            }
        }
        let mut out = Vec::with_capacity(cell * cell * 4);
        for y in 0..cell {
            for x in 0..cell {
                let sx = x.saturating_sub(inset).min(size - 1);
                let sy = y.saturating_sub(inset).min(size - 1);
                out.extend_from_slice(&texels[sy * size + sx]);
            }
        }
        Ok(out)
    }
    pub fn bytes(&self) -> u64 {
        (0..self.levels)
            .map(|l| u64::from((self.side >> l).max(1)).pow(2) * 4)
            .sum::<u64>()
            * u64::from(self.pages)
    }
}

/// Native-resolution groups keep a single large sprite from enlarging every
/// block texture. Animated layers have separate small pages: dirtying one must
/// never make the driver re-upload the static world's atlas.
pub struct AtlasSet {
    pub layouts: Vec<AtlasLayout>,
    pub locations: Vec<(usize, u32)>,
    pub offsets: Vec<usize>,
}
impl AtlasSet {
    pub fn new(sizes: &[u32], animated: &[u32], max_texture: u32) -> Result<Self, String> {
        let mut groups = std::collections::BTreeMap::<(bool, u32), Vec<u32>>::new();
        for (layer, &size) in sizes.iter().enumerate() {
            groups
                .entry((animated.contains(&(layer as u32)), size))
                .or_default()
                .push(layer as u32);
        }
        let mut result = Self {
            layouts: Vec::new(),
            locations: vec![(0, 0); sizes.len()],
            offsets: Vec::new(),
        };
        let mut pages = 0;
        for ((dynamic, size), layers) in groups {
            let cap = max_texture
                .min(if dynamic { 256 } else { 1024 })
                .max((size * 2).min(max_texture));
            let layout =
                AtlasLayout::new(size, layers.len() as u32, cap, u64::from(cap).pow(2) * 4)?;
            for (local, layer) in layers.into_iter().enumerate() {
                result.locations[layer as usize] = (result.layouts.len(), local as u32);
            }
            result.offsets.push(pages);
            pages += layout.pages as usize;
            result.layouts.push(layout);
        }
        Ok(result)
    }
    pub fn uv(&self, layer: u32, uv: [f32; 2]) -> Result<(usize, [f32; 2]), String> {
        let &(group, local) = self
            .locations
            .get(layer as usize)
            .ok_or("D3D9 sprite layer outside atlas")?;
        let (page, uv) = self.layouts[group].uv(local, uv)?;
        Ok((self.offsets[group] + page, uv))
    }
    pub fn bytes(&self) -> u64 {
        self.layouts.iter().map(AtlasLayout::bytes).sum()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sprites_and_animation_do_not_expand_or_dirty_the_static_atlas() {
        let mut sizes = vec![16; 1300];
        sizes.push(64);
        let a = AtlasSet::new(&sizes, &[0, 1], 16384).unwrap();
        assert!(a.bytes() < 12 * 1024 * 1024);
        let (animated, _) = a.uv(0, [0.0; 2]).unwrap();
        let (still, _) = a.uv(2, [0.0; 2]).unwrap();
        assert_ne!(animated, still);
        let (g, _) = a.locations[0];
        assert!(a.layouts[g].side <= 256);
        assert!(a.uv(1301, [0.0; 2]).is_err());
        // Different sprites never overlap, even across native size groups.
        let mut rectangles = std::collections::HashSet::new();
        for layer in 0..1301 {
            let (page, uv) = a.uv(layer, [0.0; 2]).unwrap();
            assert!(rectangles.insert((page, uv[0].to_bits(), uv[1].to_bits())));
        }
    }
    #[test]
    fn pages_and_uv_stay_within_caps_and_keep_distinct_sprites() {
        let a = AtlasLayout::new(16, 10, 64, 64 * 64 * 4).unwrap();
        assert_eq!((a.side, a.pages), (64, 3));
        assert_eq!(a.placement(4).unwrap(), (1, 0, 0));
        assert_eq!(a.uv(0, [0.0, 1.0]).unwrap(), (0, [0.125, 0.375]));
        assert!(a.placement(10).is_err());
        assert!(AtlasLayout::new(128, 1, 128, 1 << 20).is_err());
    }
    #[test]
    fn mip_gutters_extrude_the_sprite_and_preserve_alpha_color() {
        let a = AtlasLayout::new(2, 2, 16, 1024).unwrap();
        let pixels = [255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 255, 0, 0, 255, 0];
        let base = a.tile(0, &pixels).unwrap();
        assert_eq!(&base[..4], &[255, 0, 0, 255]);
        assert_eq!(&base[12..16], &[0, 0, 0, 0]);
        let mip = a.tile(1, &pixels).unwrap();
        for p in mip.chunks_exact(4) {
            assert_eq!(p, [127, 127, 0, 127]);
        }
    }
}
