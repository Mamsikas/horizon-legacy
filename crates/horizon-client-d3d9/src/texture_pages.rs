//! Basic alpha batches: one tight vertex range per texture page, not per quad.
use crate::Vertex;

pub struct TextureRange {
    pub page: usize,
    pub first: u32,
    pub count: u32,
    center: [f32; 3],
}

/// Append page buckets without changing vertex attributes or winding. Keep
/// page-local geometry contiguous so HAL software VP transforms each vertex
/// once and can use the shared quad IB instead of sparse sorted index buffers.
pub fn append_texture_pages(
    pages: &[Vec<Vertex>],
    vertices: &mut Vec<Vertex>,
) -> Result<Vec<TextureRange>, String> {
    let count = pages.iter().try_fold(vertices.len(), |n, page| {
        page.len()
            .is_multiple_of(4)
            .then(|| n.checked_add(page.len()))
            .flatten()
    });
    if count.is_none_or(|n| n > u32::MAX as usize) || !vertices.len().is_multiple_of(4) {
        return Err("D3D9 texture pages require bounded complete quads".into());
    }
    let mut ranges = Vec::new();
    for (page, input) in pages.iter().enumerate().filter(|(_, p)| !p.is_empty()) {
        let mut lo = input[0].pos;
        let mut hi = lo;
        for v in input {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(v.pos[axis]);
                hi[axis] = hi[axis].max(v.pos[axis]);
            }
        }
        ranges.push(TextureRange {
            page,
            first: vertices.len() as u32,
            count: input.len() as u32,
            center: std::array::from_fn(|axis| (lo[axis] + hi[axis]) * 0.5),
        });
        vertices.extend_from_slice(input);
    }
    Ok(ranges)
}

/// Section order is handled by the shared visibility list. Within a section,
/// order the few texture pages by their bounds, without quad sorts/IB uploads.
#[derive(Default)]
pub struct PageOrder {
    order: Vec<usize>,
    viewpoint: Option<[i32; 3]>,
}
impl PageOrder {
    pub fn bytes(&self) -> usize {
        self.order.capacity() * size_of::<usize>()
    }
    pub fn update(
        &mut self,
        pages: &[TextureRange],
        camera: [f64; 3],
        origin: [i32; 3],
    ) -> &[usize] {
        let block =
            std::array::from_fn(|axis| (camera[axis] - f64::from(origin[axis])).floor() as i32);
        if self.viewpoint != Some(block) || self.order.len() != pages.len() {
            if self.order.len() != pages.len() {
                self.order.clear();
                self.order.extend(0..pages.len());
            }
            let distance = |at: usize| {
                (0..3)
                    .map(|axis| {
                        let d = f64::from(pages[at].center[axis]) + f64::from(origin[axis])
                            - camera[axis];
                        d * d
                    })
                    .sum::<f64>()
            };
            self.order
                .sort_unstable_by(|&a, &b| distance(b).total_cmp(&distance(a)).then(a.cmp(&b)));
            self.viewpoint = Some(block);
        }
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alternating_alpha_pages_keep_tight_draw_ranges_and_exact_vertices() {
        let mut pages: Vec<Vec<Vertex>> = (0..4).map(|_| Vec::new()).collect();
        let mut expected = Vec::new();
        // An interleaved scene used to turn these into thousands of draw calls.
        for quad in 0..4096u32 {
            let page = (quad % 3) as usize;
            for corner in 0..4u32 {
                let v = Vertex {
                    pos: [page as f32 * 4.0, (corner & 1) as f32, (corner >> 1) as f32],
                    color: 0x80123400 | (quad % 256),
                    uv: [corner as f32 * 0.25, quad as f32 / 8192.0],
                    light: [0.03125, 0.96875],
                };
                pages[page].push(v);
            }
        }
        let prefix = pages[0][..4].to_vec();
        let mut vertices = prefix.clone();
        let ranges = append_texture_pages(&pages, &mut vertices).unwrap();
        assert_eq!(ranges.len(), 3); // one draw/page unless a caps-sized VB splits it
        expected.extend_from_slice(&prefix);
        for (range, input) in ranges.iter().zip(&pages) {
            assert_eq!(range.first as usize, expected.len());
            assert_eq!(range.count as usize, input.len());
            expected.extend_from_slice(input);
        }
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&vertices),
            bytemuck::cast_slice::<_, u8>(&expected)
        );
        let mut order = PageOrder::default();
        assert_eq!(order.update(&ranges, [-8.0, 0.0, 0.0], [0; 3]), &[2, 1, 0]);
        assert_eq!(order.update(&ranges, [20.0, 0.0, 0.0], [0; 3]), &[0, 1, 2]);
        assert_eq!(
            order.update(&ranges, [20.0, 0.0, 0.0], [128, 0, 0]),
            &[2, 1, 0]
        );
        // Native caps may split a section across VBs. Splits add bounded tight
        // ranges; they must not restore per-quad calls or transform holes.
        let mut upload = crate::UploadBatch::new(1024).unwrap();
        upload
            .push(0u8, 0u8, &crate::EncodedVertices::new(&vertices, false))
            .unwrap();
        let gpu = upload.into_pages();
        let mut draws = 0;
        let mut drawn = 0;
        for range in &ranges {
            for page in &gpu {
                for part in &page.parts {
                    let start = range.first.max(part.source.start);
                    let end = (range.first + range.count).min(part.source.end);
                    if start < end {
                        draws += 1;
                        drawn += end - start;
                        assert_eq!((start % 4, (end - start) % 4), (0, 0));
                    }
                }
            }
        }
        assert_eq!(draws, 19); // 17 native pages + two texture boundaries
        assert_eq!(drawn, 4096 * 4);
        pages[0].pop();
        assert!(append_texture_pages(&pages, &mut vertices).is_err());
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&vertices),
            bytemuck::cast_slice::<_, u8>(&expected)
        );
    }
}
