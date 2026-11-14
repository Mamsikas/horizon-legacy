//! Atlas conversion and shadow encoding shared by synchronous edits and the bounded worker.
use super::*;
#[derive(Default)]
pub(in super::super) struct Scratch {
    groups: Vec<Vec<dx::Vertex>>,
    vertices: Vec<dx::Vertex>,
    shadow: Vec<u8>,
}
pub(in super::super) struct Converted {
    pub solid: Vec<Packet>,
    pub translucent: Vec<dx::TextureRange>,
    pub shadow: Arc<dx::MeshData>,
    pub sides: crate::mesher::Sides,
    pub blocks: Vec<([i32; 3], u32)>,
    pub bounds: Option<[Vec3; 2]>,
    pub visibility: u64,
    pub gpu: dx::EncodedVertices,
}
impl Scratch {
    pub fn bytes(&self) -> usize {
        self.groups
            .iter()
            .map(|v| v.capacity() * size_of::<dx::Vertex>())
            .sum::<usize>()
            + self.vertices.capacity() * size_of::<dx::Vertex>()
            + self.shadow.capacity()
    }
    pub fn prepare(
        &mut self,
        layout: &dx::AtlasSet,
        key: SectionKey,
        mesh: &SectionMesh,
        hardware: bool,
    ) -> std::result::Result<Converted, String> {
        let mut phases = crate::spans::laps();
        // Bucket/page/alpha indices are dense and fixed by this resource set.
        // Reuse their vectors instead of allocating tree nodes and buffers per section.
        let pages = layout
            .layouts
            .iter()
            .map(|p| p.pages as usize)
            .sum::<usize>();
        self.groups.resize_with(7 * pages * 2, Vec::new);
        for group in &mut self.groups {
            group.clear();
        }
        let sides = if mesh.sides.quads() == mesh.solid.len() as u32 / 4 {
            mesh.sides
        } else {
            crate::mesher::Sides::unsorted(mesh.solid.len() as u32 / 4)
        };
        let thresholds = sides.thresholds().map(|v| v.0);
        for (quad, input) in mesh.solid.chunks_exact(4).enumerate() {
            let bucket = thresholds.map_or(0, |ends| {
                ends.iter()
                    .position(|&end| (quad as u32) < end)
                    .unwrap_or(6)
            });
            atlas::sprite_quad(
                layout,
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &terrain_quad(input),
                |page, poly| {
                    let opaque = input[0].layer & crate::mesher::OPAQUE_LAYER != 0;
                    let group = &mut self.groups[(bucket * pages + page) * 2 + usize::from(opaque)];
                    atlas::quads(poly, group);
                },
            )
            .map_err(|e| e.to_string())?;
        }
        let vertices = &mut self.vertices;
        vertices.clear();
        vertices.reserve(mesh.solid.len() + mesh.translucent.len());
        let mut solid = Vec::new();
        for (i, part) in self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
        {
            let page = i / 2 % pages;
            let bucket = i / (2 * pages);
            let source = thresholds.map_or(0..mesh.solid.len() as u32 / 4, |ends| {
                if bucket == 0 {
                    0..ends[0]
                } else {
                    ends[bucket - 1]..ends[bucket]
                }
            });
            solid.push(Packet {
                page,
                opaque: i % 2 != 0,
                first: vertices.len() as u32,
                count: part.len() as u32,
                source,
            });
            vertices.extend_from_slice(part);
        }
        for group in &mut self.groups {
            group.clear();
        }
        for input in mesh.translucent.chunks_exact(4) {
            atlas::sprite_quad(
                layout,
                u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER),
                &terrain_quad(input),
                |page, poly| {
                    atlas::quads(poly, &mut self.groups[page]);
                },
            )
            .map_err(|e| e.to_string())?;
        }
        let translucent = dx::append_texture_pages(&self.groups[..pages], vertices)?;
        let offset = [
            (key.0.rem_euclid(8) * 16) as f32,
            (key.1.rem_euclid(8) * 16) as f32,
            (key.2.rem_euclid(8) * 16) as f32,
        ];
        for v in vertices.iter_mut() {
            for axis in 0..3 {
                v.pos[axis] += offset[axis];
            }
        }
        phases.lap("d3d9_mesh_atlas");
        let gpu = dx::EncodedVertices::new(vertices, hardware);
        phases.lap("d3d9_mesh_pack");
        let shadow = Arc::new(dx::MeshData::from_encoded(&gpu, &mut self.shadow));
        phases.lap("d3d9_mesh_shadow");
        Ok(Converted {
            solid,
            translucent,
            shadow,
            sides,
            blocks: mesh.block_entities.clone(),
            bounds: mesh.bounds,
            visibility: mesh.visibility,
            gpu,
        })
    }
}
