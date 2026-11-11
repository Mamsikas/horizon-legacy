//! Lossless CPU shadows and quad index batches, independent of COM/Windows.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    /// D3DCOLOR (ARGB).
    pub color: u32,
    pub uv: [f32; 2],
    pub light: [f32; 2],
}

pub struct MeshData {
    bytes: Vec<u8>,
    vertices: usize,
    compressed: bool,
}
impl MeshData {
    pub fn new(vertices: &[Vertex]) -> Self {
        let raw = bytemuck::cast_slice(vertices);
        let compressed = lz4_flex::block::compress(raw);
        let use_compressed = compressed.len() < raw.len();
        Self {
            bytes: if use_compressed {
                compressed
            } else {
                raw.to_vec()
            },
            vertices: vertices.len(),
            compressed: use_compressed,
        }
    }
    pub fn len(&self) -> usize {
        self.vertices
    }
    pub fn is_empty(&self) -> bool {
        self.vertices == 0
    }
    pub fn bytes(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn decode(&self, out: &mut Vec<Vertex>) -> Result<(), String> {
        out.resize(self.vertices, bytemuck::Zeroable::zeroed());
        let dest = bytemuck::cast_slice_mut(out);
        if self.compressed {
            let n =
                lz4_flex::block::decompress_into(&self.bytes, dest).map_err(|e| e.to_string())?;
            if n != dest.len() {
                return Err("D3D9 mesh shadow length mismatch".into());
            }
        } else {
            dest.copy_from_slice(&self.bytes);
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct IndexBatch {
    indices: Vec<u16>,
    end: u32,
    start: u32,
}
impl IndexBatch {
    pub fn clear(&mut self) {
        self.indices.clear();
        self.end = 0;
        self.start = 0;
    }
    pub fn append(&mut self, first: u32, count: u32) -> Result<(), String> {
        let end = first
            .checked_add(count)
            .ok_or("D3D9 index range overflow")?;
        if first % 4 != 0 || count % 4 != 0 || end > 65536 {
            return Err("D3D9 invalid quad index range".into());
        }
        if count > 0 {
            self.start = if self.indices.is_empty() {
                first
            } else {
                self.start.min(first)
            };
        }
        if count == 0 {
            return Ok(());
        }
        for base in (first..end).step_by(4) {
            let b = base as u16;
            self.indices
                .extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        self.end = self.end.max(end);
        Ok(())
    }
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }
    pub fn start_vertex(&self) -> u32 {
        self.start
    }
    pub fn end_vertex(&self) -> u32 {
        self.end
    }
    pub fn bytes(&self) -> usize {
        self.indices.capacity() * 2
    }
}

/// Append until full; DISCARD only starts a new allocation, never every draw.
#[derive(Default)]
pub struct Ring {
    cursor: usize,
}
impl Ring {
    pub fn reserve(&mut self, length: usize, capacity: usize) -> Option<(usize, bool)> {
        if length > capacity {
            return None;
        }
        let discard = self.cursor == 0 || length > capacity.saturating_sub(self.cursor);
        let offset = if discard { 0 } else { self.cursor };
        self.cursor = offset + length;
        Some((offset, discard))
    }
    pub fn reset(&mut self) {
        self.cursor = 0;
    }
}

/// Preserve a complete interval: mixing the start of disabled atmospheric fog
/// with the end of distance fog incorrectly fills the whole scene with haze.
pub fn fog_interval(environment: (f32, f32), distance: (f32, f32)) -> (f32, f32) {
    if environment.1 < distance.1 || (environment.1 == distance.1 && environment.0 < distance.0) {
        environment
    } else {
        distance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shadows_preserve_every_vertex_bit_and_reuse_decode_storage() {
        let vertices: Vec<_> = (0..4096)
            .map(|i| Vertex {
                pos: [(i % 16) as f32, (i / 16 % 16) as f32, 0.0],
                color: 0xff204080,
                uv: [0.125, 0.25],
                light: [0.03125, 0.96875],
            })
            .collect();
        let data = MeshData::new(&vertices);
        assert!(data.bytes() < vertices.len() as u64 * 8);
        let mut decoded = Vec::new();
        data.decode(&mut decoded).unwrap();
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&decoded),
            bytemuck::cast_slice::<_, u8>(&vertices)
        );
        let capacity = decoded.capacity();
        data.decode(&mut decoded).unwrap();
        assert_eq!(decoded.capacity(), capacity);
        let bits = Vertex {
            pos: [f32::from_bits(0x7fc01234), -0.0, f32::INFINITY],
            color: 123,
            uv: [-1.5, 2.0],
            light: [0.0, 1.0],
        };
        MeshData::new(&[bits]).decode(&mut decoded).unwrap();
        assert_eq!(bytemuck::bytes_of(&decoded[0]), bytemuck::bytes_of(&bits));
        MeshData::new(&[]).decode(&mut decoded).unwrap();
        assert!(decoded.is_empty());
    }
    #[test]
    fn disjoint_visible_quad_ranges_batch_without_drawing_hidden_faces() {
        let mut b = IndexBatch::default();
        b.append(8, 4).unwrap();
        b.append(20, 8).unwrap();
        assert_eq!(
            b.indices(),
            [
                8, 9, 10, 8, 10, 11, 20, 21, 22, 20, 22, 23, 24, 25, 26, 24, 26, 27
            ]
        );
        assert_eq!(b.end_vertex(), 28);
        assert_eq!(b.start_vertex(), 8);
        assert!(b.append(65532, 8).is_err());
        assert!(b.append(2, 4).is_err());
        b.clear();
        assert!(b.indices().is_empty());
        assert_eq!(b.end_vertex(), 0);
    }
    #[test]
    fn stream_ring_appends_and_only_discards_at_wrap() {
        let mut r = Ring::default();
        assert_eq!(r.reserve(4, 16), Some((0, true)));
        assert_eq!(r.reserve(8, 16), Some((4, false)));
        assert_eq!(r.reserve(4, 16), Some((12, false)));
        assert_eq!(r.reserve(4, 16), Some((0, true)));
        assert_eq!(r.reserve(17, 16), None);
        assert_eq!(r.reserve(4, 16), Some((4, false)));
        r.reset();
        assert_eq!(r.reserve(8, 16), Some((0, true)));
    }
    #[test]
    fn distant_atmosphere_does_not_move_near_distance_fog_to_the_camera() {
        assert_eq!(fog_interval((-8.0, 1e9), (230.0, 256.0)), (230.0, 256.0));
        assert_eq!(fog_interval((-8.0, 96.0), (230.0, 256.0)), (-8.0, 96.0));
        assert_eq!(fog_interval((30.0, 180.0), (230.0, 256.0)), (30.0, 180.0));
        assert_eq!(fog_interval((64.0, 256.0), (230.0, 256.0)), (64.0, 256.0));
    }
}

/// Reordering is safe only for disjoint screen bounds; overlapping pages are
/// separated by a painter-order barrier. Same-page primitives retain their order.
#[derive(Default)]
pub struct PaintWindow {
    bounds: Vec<(usize, [f32; 4])>,
}
impl PaintWindow {
    pub fn barrier(&mut self, page: usize, rect: [f32; 4]) -> bool {
        let flush = self.bounds.len() >= 128
            || self.bounds.iter().any(|(p, r)| {
                *p != page && r[0] < rect[2] && rect[0] < r[2] && r[1] < rect[3] && rect[1] < r[3]
            });
        if flush {
            self.bounds.clear();
        }
        self.bounds.push((page, rect));
        flush
    }
}

#[cfg(test)]
mod paint_tests {
    use super::*;
    #[test]
    fn independent_glyphs_group_but_overlapping_alpha_keeps_painter_order() {
        let mut window = PaintWindow::default();
        for x in 0..64 {
            assert!(!window.barrier(x % 4, [x as f32 * 2.0, 0.0, x as f32 * 2.0 + 1.0, 1.0]));
        }
        assert!(window.barrier(1, [0.0, 0.0, 1.0, 1.0]));
        assert!(!window.barrier(1, [0.0, 0.0, 1.0, 1.0]));
        assert!(window.barrier(2, [0.0, 0.0, 1.0, 1.0]));
    }
    #[test]
    fn reordered_alpha_rectangles_match_the_original_image() {
        let mut seed = 12345u32;
        let mut rects = Vec::new();
        for _ in 0..1000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = seed % 24;
            let y = seed >> 8 & 23;
            let w = (seed >> 16 & 7) + 1;
            rects.push(((seed >> 24) as usize % 4, [x, y, x + w, (y + 3).min(32)]));
        }
        let mut reordered = Vec::new();
        let mut open = Vec::<(usize, Vec<usize>)>::new();
        let mut window = PaintWindow::default();
        let flush = |open: &mut Vec<(usize, Vec<usize>)>, out: &mut Vec<usize>| {
            for (_, indices) in open.drain(..) {
                out.extend(indices);
            }
        };
        for (i, &(page, rect)) in rects.iter().enumerate() {
            if window.barrier(page, rect.map(|v| v as f32)) {
                flush(&mut open, &mut reordered);
            }
            if let Some((_, group)) = open.iter_mut().find(|(p, _)| *p == page) {
                group.push(i);
            } else {
                open.push((page, vec![i]));
            }
        }
        flush(&mut open, &mut reordered);
        let render = |order: Vec<usize>| {
            let mut pixels = vec![[0.0f32; 3]; 32 * 32];
            for i in order {
                let (page, [x0, y0, x1, y1]) = rects[i];
                let color = [page as f32 / 3.0, (i % 7) as f32 / 7.0, 0.25];
                for y in y0..y1 {
                    for x in x0..x1 {
                        for c in 0..3 {
                            let p = &mut pixels[(y * 32 + x) as usize][c];
                            *p = *p * 0.6 + color[c] * 0.4;
                        }
                    }
                }
            }
            pixels
        };
        assert_eq!(render((0..rects.len()).collect()), render(reordered));
    }
}
