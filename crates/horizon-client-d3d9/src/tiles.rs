//! Sparse CPU backing for a legacy 2D atlas. GPU placement is independent of
//! virtual atlas coordinates; immutable tiles compress, patched tiles stay raw.
enum Tile {
    Empty,
    Packed(Vec<u8>),
    Raw(Vec<u8>),
}
pub struct TileImage {
    pub width: u32,
    pub height: u32,
    pub tile: u32,
    pub padding: u32,
    pub columns: u32,
    pub rows: u32,
    tiles: Vec<Tile>,
}
impl TileImage {
    pub fn new(
        rgba: &[u8],
        width: u32,
        height: u32,
        tile: u32,
        padding: u32,
    ) -> Result<Self, String> {
        if width == 0
            || height == 0
            || tile == 0
            || padding > tile
            || (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4))
                != Some(rgba.len())
        {
            return Err("D3D9 invalid tiled image dimensions".into());
        }
        let columns = width.div_ceil(tile);
        let rows = height.div_ceil(tile);
        let cell = tile + padding * 2;
        let mut tiles = Vec::new();
        for y in 0..rows {
            for x in 0..columns {
                let mut pixels = Vec::with_capacity((cell * cell * 4) as usize);
                for cy in 0..cell {
                    let sy = (i64::from(y * tile + cy) - i64::from(padding))
                        .clamp(0, i64::from(height - 1)) as u32;
                    for cx in 0..cell {
                        let sx = (i64::from(x * tile + cx) - i64::from(padding))
                            .clamp(0, i64::from(width - 1)) as u32;
                        let at = ((sy * width + sx) * 4) as usize;
                        pixels.extend_from_slice(&rgba[at..at + 4]);
                    }
                }
                if pixels.iter().all(|&b| b == 0) {
                    tiles.push(Tile::Empty);
                } else {
                    let packed = lz4_flex::block::compress(&pixels);
                    tiles.push(if packed.len() < pixels.len() {
                        Tile::Packed(packed)
                    } else {
                        Tile::Raw(pixels)
                    });
                }
            }
        }
        Ok(Self {
            width,
            height,
            tile,
            padding,
            columns,
            rows,
            tiles,
        })
    }
    pub fn cell(&self) -> u32 {
        self.tile + self.padding * 2
    }
    pub fn len(&self) -> usize {
        self.tiles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
    pub fn placement(&self, slot: u32, side: u32) -> Option<(usize, u32, u32)> {
        let columns = side / self.cell();
        if columns == 0 {
            return None;
        }
        Some((
            (slot / (columns * columns)) as usize,
            (slot % columns) * self.cell(),
            (slot / columns % columns) * self.cell(),
        ))
    }
    pub fn present(&self, id: usize) -> bool {
        !matches!(self.tiles[id], Tile::Empty)
    }
    pub fn bytes(&self) -> usize {
        self.tiles.capacity() * size_of::<Tile>()
            + self
                .tiles
                .iter()
                .map(|t| match t {
                    Tile::Empty => 0,
                    Tile::Packed(v) | Tile::Raw(v) => v.capacity(),
                })
                .sum::<usize>()
    }
    pub fn decode(&self, id: usize, out: &mut Vec<u8>) -> Result<(), String> {
        out.resize((self.cell() * self.cell() * 4) as usize, 0);
        match &self.tiles[id] {
            Tile::Empty => out.fill(0),
            Tile::Packed(bytes) => {
                let len =
                    lz4_flex::block::decompress_into(bytes, out).map_err(|e| e.to_string())?;
                if len != out.len() {
                    return Err("D3D9 tile shadow length mismatch".into());
                }
            }
            Tile::Raw(bytes) => out.copy_from_slice(bytes),
        }
        Ok(())
    }
    /// Patch virtual texels and both sides of a tile seam, including the
    /// clamp-to-edge gutters at the outer image boundary.
    pub fn patch(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> Result<Vec<usize>, String> {
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        if x.checked_add(w).is_none_or(|n| n > self.width)
            || y.checked_add(h).is_none_or(|n| n > self.height)
            || rgba.len() != w as usize * h as usize * 4
        {
            return Err("D3D9 tiled patch outside atlas".into());
        }
        let mut dirty = Vec::new();
        let cell = self.cell();
        for py in y.saturating_sub(self.padding) / self.tile
            ..=((y + h - 1 + self.padding).min(self.height - 1) / self.tile)
        {
            for px in x.saturating_sub(self.padding) / self.tile
                ..=((x + w - 1 + self.padding).min(self.width - 1) / self.tile)
            {
                let id = (py * self.columns + px) as usize;
                if !matches!(self.tiles[id], Tile::Raw(_)) {
                    let mut raw = Vec::new();
                    self.decode(id, &mut raw)?;
                    self.tiles[id] = Tile::Raw(raw);
                }
                let Tile::Raw(raw) = &mut self.tiles[id] else {
                    unreachable!()
                };
                for cy in 0..cell {
                    let sy = (i64::from(py * self.tile + cy) - i64::from(self.padding))
                        .clamp(0, i64::from(self.height - 1)) as u32;
                    if sy < y || sy >= y + h {
                        continue;
                    }
                    for cx in 0..cell {
                        let sx = (i64::from(px * self.tile + cx) - i64::from(self.padding))
                            .clamp(0, i64::from(self.width - 1))
                            as u32;
                        if sx < x || sx >= x + w {
                            continue;
                        }
                        let src = (((sy - y) * w + sx - x) * 4) as usize;
                        let dst = ((cy * cell + cx) * 4) as usize;
                        raw[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                    }
                }
                dirty.push(id);
            }
        }
        Ok(dirty)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_tiles_fill_rows_then_pages_without_overlapping() {
        let image = TileImage::new(&[255; 64], 4, 4, 4, 1).unwrap();
        assert_eq!(image.placement(0, 12), Some((0, 0, 0)));
        assert_eq!(image.placement(1, 12), Some((0, 6, 0)));
        assert_eq!(image.placement(2, 12), Some((0, 0, 6)));
        assert_eq!(image.placement(4, 12), Some((1, 0, 0)));
        let slots: std::collections::HashSet<_> =
            (0..100).map(|i| image.placement(i, 12).unwrap()).collect();
        assert_eq!(slots.len(), 100);
    }
    #[test]
    fn virtual_tiles_and_patches_match_the_source_across_seams_and_outer_gutters() {
        let (w, h) = (13u32, 9u32);
        let mut source: Vec<_> = (0..w * h)
            .flat_map(|n| [n as u8, (n * 3) as u8, 77, if n % 3 == 0 { 0 } else { 255 }])
            .collect();
        let mut image = TileImage::new(&source, w, h, 4, 1).unwrap();
        let check = |image: &TileImage, source: &[u8]| {
            let mut decoded = Vec::new();
            for py in 0..image.rows {
                for px in 0..image.columns {
                    image
                        .decode((py * image.columns + px) as usize, &mut decoded)
                        .unwrap();
                    for y in 0..6 {
                        for x in 0..6 {
                            let sx = (px as i32 * 4 + x - 1).clamp(0, w as i32 - 1) as usize;
                            let sy = (py as i32 * 4 + y - 1).clamp(0, h as i32 - 1) as usize;
                            assert_eq!(
                                &decoded[((y * 6 + x) * 4) as usize..][..4],
                                &source[(sy * w as usize + sx) * 4..][..4]
                            );
                        }
                    }
                }
            }
        };
        check(&image, &source);
        for (x, y, pw, ph) in [(3, 3, 3, 3), (0, 0, 2, 2), (11, 7, 2, 2)] {
            let patch = vec![91u8; (pw * ph * 4) as usize];
            image.patch(x, y, pw, ph, &patch).unwrap();
            for row in y..y + ph {
                let at = ((row * w + x) * 4) as usize;
                source[at..at + (pw * 4) as usize].copy_from_slice(&patch[..(pw * 4) as usize]);
            }
            check(&image, &source);
        }
    }
}
