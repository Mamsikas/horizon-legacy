//! Atlas conversion and shadow encoding shared by synchronous edits and the bounded worker.
use super::*;
mod honey;
#[derive(Default)]
pub(in super::super) struct Scratch {
    groups: Vec<Vec<dx::Vertex>>,
    vertices: Vec<dx::Vertex>,
    shadow: Vec<u8>,
    transforms: Vec<dx::AtlasUv>,
    honey: Option<honey::Layers>,
    cull_solid: bool,
}
pub(in super::super) struct Converted {
    pub solid: Vec<Packet>,
    pub solid_bounds: Option<Box<SolidBounds>>,
    pub translucent: Vec<dx::TextureRange>,
    pub shadow: Arc<dx::MeshData>,
    pub sides: crate::mesher::Sides,
    pub blocks: Vec<([i32; 3], u32)>,
    pub bounds: Option<[Vec3; 2]>,
    pub visibility: u64,
    pub gpu: dx::EncodedVertices,
}
impl Scratch {
    pub(in super::super) fn new(honey: Option<[u16; 3]>, cull_solid: bool) -> Self {
        Self {
            honey: honey.map(honey::Layers),
            cull_solid,
            ..Self::default()
        }
    }
    pub fn bytes(&self) -> usize {
        self.groups
            .iter()
            .map(|v| v.capacity() * size_of::<dx::Vertex>())
            .sum::<usize>()
            + self.vertices.capacity() * size_of::<dx::Vertex>()
            + self.shadow.capacity()
            + self.transforms.capacity() * size_of::<dx::AtlasUv>()
    }
    pub fn prepare(
        &mut self,
        layout: &dx::AtlasSet,
        key: SectionKey,
        mesh: &SectionMesh,
        hardware: bool,
        defer_shadow: bool,
    ) -> std::result::Result<Converted, String> {
        let mut phases = crate::spans::laps();
        if self.transforms.is_empty() {
            self.transforms = (0..layout.locations.len())
                .map(|layer| layout.transform(layer as u32))
                .collect::<std::result::Result<_, _>>()?;
        }
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
        let mut bucket = 0;
        for (quad, input) in mesh.solid.as_chunks::<4>().0.iter().enumerate() {
            if let Some(ends) = thresholds {
                while bucket < 6 && quad as u32 >= ends[bucket] {
                    bucket += 1;
                }
            }
            let layer = u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER);
            let opaque = usize::from(input[0].layer & crate::mesher::OPAQUE_LAYER != 0);
            let mut vertices = terrain_quad(input);
            if input
                .iter()
                .all(|v| v.uv.iter().all(|&u| u <= crate::mesher::UV_STEPS as u16))
                // A quad collapsed at UV=1 wraps in the general path; keep that behavior.
                && input.iter().any(|v| v.uv.iter().all(|&u| u < crate::mesher::UV_STEPS as u16))
            {
                let map = *self
                    .transforms
                    .get(layer as usize)
                    .ok_or("D3D9 sprite layer outside atlas")?;
                for v in &mut vertices {
                    v.uv = map.map(v.uv);
                }
                self.groups[(bucket * pages + map.page) * 2 + opaque].extend_from_slice(&vertices);
            } else {
                atlas::sprite_quad(layout, layer, &vertices, |page, poly| {
                    atlas::quads(poly, &mut self.groups[(bucket * pages + page) * 2 + opaque]);
                })
                .map_err(|e| e.to_string())?;
            }
        }
        let vertices = &mut self.vertices;
        vertices.clear();
        vertices.reserve(mesh.solid.len() + mesh.translucent.len());
        let offset = Vec3::new(
            (key.0.rem_euclid(8) * 16) as f32,
            (key.1.rem_euclid(8) * 16) as f32,
            (key.2.rem_euclid(8) * 16) as f32,
        );
        let mut solid_bounds = (self.cull_solid && !mesh.solid.is_empty())
            .then(|| Box::new([[Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)]; 7]));
        let mut solid = Vec::new();
        for (i, part) in self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
        {
            let page = i / 2 % pages;
            let bucket = i / (2 * pages);
            if let Some(bounds) = &mut solid_bounds {
                let [lo, hi] = &mut bounds[bucket];
                if !lo.x.is_nan() {
                    for vertex in part {
                        let pos = Vec3::from_array(vertex.pos) + offset;
                        if !pos.is_finite() {
                            *lo = Vec3::splat(f32::NAN);
                            break;
                        }
                        *lo = lo.min(pos);
                        *hi = hi.max(pos);
                    }
                }
            }
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
                bucket: bucket as u8,
            });
            vertices.extend_from_slice(part);
        }
        for group in &mut self.groups {
            group.clear();
        }
        let mut append_translucent = |input: &[TerrainVertex; 4]| {
            let layer = u32::from(input[0].layer & !crate::mesher::OPAQUE_LAYER);
            let mut quad = terrain_quad(input);
            if input
                .iter()
                .all(|v| v.uv.iter().all(|&u| u <= crate::mesher::UV_STEPS as u16))
                // A quad collapsed at UV=1 wraps in the general path; keep that behavior.
                && input.iter().any(|v| v.uv.iter().all(|&u| u < crate::mesher::UV_STEPS as u16))
            {
                let map = *self
                    .transforms
                    .get(layer as usize)
                    .ok_or("D3D9 sprite layer outside atlas")?;
                for v in &mut quad {
                    v.uv = map.map(v.uv);
                }
                self.groups[map.page].extend_from_slice(&quad);
            } else {
                atlas::sprite_quad(layout, layer, &quad, |page, poly| {
                    atlas::quads(poly, &mut self.groups[page]);
                })
                .map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(())
        };
        let quads = mesh.translucent.as_chunks::<4>().0;
        if let Some(honey) = self.honey {
            honey.in_draw_order(quads, append_translucent)?;
        } else {
            for input in quads {
                append_translucent(input)?;
            }
        }
        let translucent = dx::append_texture_pages(&self.groups[..pages], vertices)?;
        for v in vertices.iter_mut() {
            for axis in 0..3 {
                v.pos[axis] += offset[axis];
            }
        }
        phases.lap("d3d9_mesh_atlas");
        let gpu = dx::EncodedVertices::new(vertices, hardware);
        phases.lap("d3d9_mesh_pack");
        let shadow = Arc::new(if defer_shadow {
            dx::MeshData::pending(&gpu)
        } else {
            dx::MeshData::from_encoded(&gpu, &mut self.shadow)
        });
        phases.lap("d3d9_mesh_shadow");
        Ok(Converted {
            solid,
            solid_bounds,
            translucent,
            shadow,
            sides,
            blocks: mesh.block_entities.clone(),
            bounds: mesh.bounds,
            visibility: mesh.visibility,
            gpu,
        })
    }
    pub(super) fn compress(&mut self, source: &dx::MeshData) -> Option<dx::MeshData> {
        source.compress_pending(&mut self.shadow)
    }
}
