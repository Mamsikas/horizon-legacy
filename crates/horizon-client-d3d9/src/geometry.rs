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
    encoding: crate::VertexEncoding,
}
impl MeshData {
    pub fn new(vertices: &[Vertex]) -> Self {
        Self::with_scratch(vertices, &mut Vec::new())
    }
    /// Reuse the encoder's temporary storage across a burst of section updates.
    pub fn with_scratch(vertices: &[Vertex], shuffled: &mut Vec<u8>) -> Self {
        let raw: &[u8] = bytemuck::cast_slice(vertices);
        Self::encode(raw, vertices.len(), crate::VertexEncoding::Full, shuffled)
    }
    pub fn from_encoded(vertices: &crate::EncodedVertices, scratch: &mut Vec<u8>) -> Self {
        Self::encode(
            vertices.bytes(),
            vertices.len(),
            vertices.encoding(),
            scratch,
        )
    }
    fn encode(
        raw: &[u8],
        count: usize,
        encoding: crate::VertexEncoding,
        shuffled: &mut Vec<u8>,
    ) -> Self {
        // Vertex deltas have repeated colours/light and short position/UV
        // changes. Byte planes let LZ4 see those runs without losing one bit
        // (including signed zero/NaN payloads). Small blocks bound decode scratch.
        shuffled.clear();
        shuffled.extend_from_slice(raw);
        let stride = encoding.stride();
        for i in (stride..shuffled.len()).rev() {
            shuffled[i] ^= shuffled[i - stride];
        }
        shuffle(shuffled, stride, false);
        let compressed = lz4_flex::block::compress(shuffled);
        let use_compressed = compressed.len() < raw.len();
        Self {
            bytes: if use_compressed {
                compressed
            } else {
                raw.to_vec()
            },
            vertices: count,
            compressed: use_compressed,
            encoding,
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
    pub fn encoding(&self) -> crate::VertexEncoding {
        self.encoding
    }
    pub fn decode(&self, out: &mut Vec<Vertex>) -> Result<(), String> {
        out.resize(self.vertices, bytemuck::Zeroable::zeroed());
        let stride = self.encoding.stride();
        let dest = &mut bytemuck::cast_slice_mut::<_, u8>(out)[..self.vertices * stride];
        if self.compressed {
            let n =
                lz4_flex::block::decompress_into(&self.bytes, dest).map_err(|e| e.to_string())?;
            if n != dest.len() {
                return Err("D3D9 mesh shadow length mismatch".into());
            }
            shuffle(dest, stride, true);
            for i in stride..dest.len() {
                dest[i] ^= dest[i - stride];
            }
        } else {
            dest.copy_from_slice(&self.bytes);
        }
        crate::terrain::expand_in_place(self.encoding, out);
        Ok(())
    }
}
fn shuffle(bytes: &mut [u8], stride: usize, inverse: bool) {
    match stride {
        20 => shuffle_fixed::<20>(bytes, inverse),
        28 => shuffle_fixed::<28>(bytes, inverse),
        32 => shuffle_fixed::<32>(bytes, inverse),
        _ => unreachable!(),
    }
}
fn shuffle_fixed<const STRIDE: usize>(bytes: &mut [u8], inverse: bool) {
    let mut scratch = [0u8; size_of::<Vertex>() * 16];
    for chunk in bytes.chunks_mut(STRIDE * 16) {
        scratch[..chunk.len()].copy_from_slice(chunk);
        let n = chunk.len() / STRIDE;
        for b in 0..STRIDE {
            for v in 0..n {
                if inverse {
                    chunk[v * STRIDE + b] = scratch[b * n + v];
                } else {
                    chunk[b * n + v] = scratch[v * STRIDE + b];
                }
            }
        }
    }
}

#[derive(Default)]
pub struct IndexBatch {
    indices: Vec<u16>,
    end: u32,
    start: u32,
    windows: Vec<IndexWindow>,
    window_at: usize,
    window_start: u32,
    window_end: u32,
}
#[derive(Clone)]
#[cfg_attr(not(windows), allow(dead_code))] // Native draws consume the recorded bounds.
pub(crate) struct IndexWindow {
    pub first: u32,
    pub count: u32,
    pub start: u32,
    pub end: u32,
}
impl IndexBatch {
    pub fn clear(&mut self) {
        self.indices.clear();
        self.end = 0;
        self.start = 0;
        self.windows.clear();
        self.window_at = 0;
        self.window_start = 0;
        self.window_end = 0;
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
        self.window_start = if self.indices.len() == self.window_at {
            first
        } else {
            self.window_start.min(first)
        };
        self.window_end = self.window_end.max(end);
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
    /// Close one ordered draw range without allocating another index buffer.
    /// Quad order inside a range is retained, including back-to-front sorting.
    pub fn finish_group(&mut self) -> Option<u32> {
        let count = self.indices.len() - self.window_at;
        if count == 0 {
            return None;
        }
        let id = self.windows.len() as u32;
        self.windows.push(IndexWindow {
            first: self.window_at as u32,
            count: count as u32,
            start: self.window_start,
            end: self.window_end,
        });
        self.window_at = self.indices.len();
        self.window_start = 0;
        self.window_end = 0;
        Some(id)
    }
    #[cfg(any(windows, test))]
    pub(crate) fn windows(&self) -> &[IndexWindow] {
        &self.windows
    }
    #[cfg(any(windows, test))]
    pub(crate) fn groups_complete(&self) -> bool {
        self.windows.is_empty() || self.window_at == self.indices.len()
    }
    pub fn start_vertex(&self) -> u32 {
        self.start
    }
    pub fn end_vertex(&self) -> u32 {
        self.end
    }
    pub fn bytes(&self) -> usize {
        self.indices.capacity() * 2 + self.windows.capacity() * size_of::<IndexWindow>()
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
    fn ordered_groups_preserve_quad_order_and_tight_per_draw_vertex_bounds() {
        let mut batch = IndexBatch::default();
        batch.append(20, 4).unwrap();
        batch.append(8, 4).unwrap();
        assert_eq!(batch.finish_group(), Some(0));
        batch.append(100, 4).unwrap();
        assert!(!batch.groups_complete());
        assert_eq!(batch.finish_group(), Some(1));
        assert!(batch.groups_complete());
        assert_eq!(
            batch.indices(),
            &[
                20, 21, 22, 20, 22, 23, 8, 9, 10, 8, 10, 11, 100, 101, 102, 100, 102, 103
            ]
        );
        assert_eq!(
            batch
                .windows()
                .iter()
                .map(|w| (w.first, w.count, w.start, w.end))
                .collect::<Vec<_>>(),
            [(0, 12, 8, 24), (12, 6, 100, 104)]
        );
        batch.clear();
        batch.append(4, 4).unwrap();
        assert_eq!(batch.finish_group(), Some(0));
        let w = &batch.windows()[0];
        assert_eq!((w.first, w.count, w.start, w.end), (0, 6, 4, 8));
    }
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
        let mut encoder = Vec::new();
        let data = MeshData::with_scratch(&vertices, &mut encoder);
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
        let encoder_capacity = encoder.capacity();
        MeshData::with_scratch(&[bits], &mut encoder)
            .decode(&mut decoded)
            .unwrap();
        assert_eq!(encoder.capacity(), encoder_capacity);
        assert_eq!(bytemuck::bytes_of(&decoded[0]), bytemuck::bytes_of(&bits));
        for length in [15, 16, 17, 31, 33] {
            let partial = vec![bits; length];
            MeshData::with_scratch(&partial, &mut encoder)
                .decode(&mut decoded)
                .unwrap();
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(&decoded),
                bytemuck::cast_slice::<_, u8>(&partial)
            );
        }
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
    pub fn bytes(&self) -> usize {
        self.bounds.capacity() * size_of::<(usize, [f32; 4])>()
    }
    pub fn clear(&mut self) {
        self.bounds.clear();
    }
    pub fn barrier(&mut self, page: usize, rect: [f32; 4]) -> bool {
        let flush = self.bounds.len() >= 128
            || self.bounds.iter().any(|(p, r)| {
                *p != page && r[0] < rect[2] && rect[0] < r[2] && r[1] < rect[3] && rect[1] < r[3]
            });
        if flush {
            self.bounds.clear();
        }
        if let Some((last_page, last)) = self.bounds.last_mut()
            && *last_page == page
        {
            last[0] = last[0].min(rect[0]);
            last[1] = last[1].min(rect[1]);
            last[2] = last[2].max(rect[2]);
            last[3] = last[3].max(rect[3]);
        } else {
            self.bounds.push((page, rect));
        }
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
        window.clear();
        for x in 0..10_000 {
            assert!(!window.barrier(1, [x as f32, 0.0, x as f32 + 1.0, 1.0]));
        }
        assert_eq!(window.bounds.len(), 1);
        assert!(window.barrier(2, [1.0, 0.0, 2.0, 1.0]));
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
