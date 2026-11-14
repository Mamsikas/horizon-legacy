//! Group already-encoded section arrivals before creating immutable native VBs.
//! No device, decoding, spare GPU capacity or change to transparent draw order.
use crate::EncodedVertices;
use std::ops::Range;

pub struct UploadPart<K> {
    pub key: K,
    pub source: Range<u32>,
    pub first: u32,
}
pub struct UploadPage<K, R> {
    pub region: R,
    pub vertices: EncodedVertices,
    pub parts: Vec<UploadPart<K>>,
}
pub struct UploadBatch<K, R> {
    pages: Vec<UploadPage<K, R>>,
    limit: usize,
    bytes: u64,
}
impl<K: Copy, R: Copy + PartialEq> UploadBatch<K, R> {
    pub fn new(limit: usize) -> Result<Self, String> {
        if limit == 0 || limit % 4 != 0 || limit > u32::MAX as usize {
            return Err("D3D9 upload page requires a bounded complete-quad capacity".into());
        }
        Ok(Self {
            pages: Vec::new(),
            limit,
            bytes: 0,
        })
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn push(&mut self, region: R, key: K, vertices: &EncodedVertices) -> Result<(), String> {
        if !vertices.len().is_multiple_of(4) || vertices.len() > u32::MAX as usize {
            return Err("D3D9 section has an incomplete quad".into());
        }
        let mut first = 0;
        while first < vertices.len() {
            let at = self.pages.iter().rposition(|page| {
                page.region == region
                    && page.vertices.encoding() == vertices.encoding()
                    && page.vertices.len() < self.limit
            });
            let at = at.unwrap_or_else(|| {
                self.pages.push(UploadPage {
                    region,
                    vertices: EncodedVertices::empty(vertices.encoding()),
                    parts: Vec::new(),
                });
                self.pages.len() - 1
            });
            let page = &mut self.pages[at];
            let count = (self.limit - page.vertices.len()).min(vertices.len() - first);
            let offset = page.vertices.len() as u32;
            page.vertices.append_range(vertices, first..first + count)?;
            page.parts.push(UploadPart {
                key,
                source: first as u32..(first + count) as u32,
                first: offset,
            });
            self.bytes += (count * vertices.encoding().stride()) as u64;
            first += count;
        }
        Ok(())
    }
    pub fn into_pages(self) -> Vec<UploadPage<K, R>> {
        self.pages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MeshData, Vertex};
    #[test]
    fn arrivals_share_caps_bounded_pages_without_changing_section_draw_indices() {
        let vertex = Vertex {
            pos: [0.0; 3],
            color: 0xff123456,
            uv: [0.125, 0.25],
            light: [0.03125, 0.96875],
        };
        let mut batch = UploadBatch::new(64).unwrap();
        let mut sources = Vec::new();
        for key in 0..36u32 {
            let mut input = vec![vertex; if key == 0 { 80 } else { 4 }];
            for (i, v) in input.iter_mut().enumerate() {
                v.pos[0] = (key * 4 + i as u32) as f32 / 128.0;
            }
            if key == 34 {
                input[0].pos[0] = 0.1;
            }
            let encoded = EncodedVertices::new(&input, key != 35);
            batch
                .push(if key == 33 { 1 } else { 0 }, key, &encoded)
                .unwrap();
            sources.push(input);
        }
        let pages = batch.into_pages();
        assert_eq!(pages.len(), 7); // four compact pages, another region, wide, FFP
        let mut restored = vec![Vec::<Vertex>::new(); sources.len()];
        let mut scratch = Vec::new();
        for page in &pages {
            assert!(page.vertices.len() <= 64);
            let mut decoded = Vec::new();
            MeshData::from_encoded(&page.vertices, &mut scratch)
                .decode(&mut decoded)
                .unwrap();
            for part in &page.parts {
                let at = part.key as usize;
                assert_eq!(restored[at].len(), part.source.start as usize);
                for base in (part.source.start..part.source.end).step_by(4) {
                    for corner in [0, 1, 2, 0, 2, 3] {
                        let gpu = (part.first + base - part.source.start + corner) as usize;
                        assert_eq!(
                            bytemuck::bytes_of(&decoded[gpu]),
                            bytemuck::bytes_of(&sources[at][(base + corner) as usize])
                        );
                    }
                }
                restored[at].extend_from_slice(
                    &decoded[part.first as usize..part.first as usize + part.source.len()],
                );
            }
        }
        for (actual, expected) in restored.iter().zip(&sources) {
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(actual),
                bytemuck::cast_slice::<_, u8>(expected)
            );
        }
        assert!(UploadBatch::<u32, u32>::new(3).is_err());
        let mut output = EncodedVertices::empty(crate::VertexEncoding::Full);
        assert!(output.append_range(&pages[0].vertices, 0..4).is_err());
        assert!(output.is_empty());
    }
}
