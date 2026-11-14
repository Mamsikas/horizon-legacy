//! Exact CPU-to-GPU encodings for the optional hardware VS2 path.
//! FFP/software-vertex devices keep the original 32-byte vertex.
use crate::Vertex;

#[cfg(any(windows, test))]
#[path = "terrain_program.rs"]
mod program;
#[cfg(windows)]
pub(crate) fn bytecode() -> &'static [u32] {
    program::WORDS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexEncoding {
    Full,
    Wide,
    Compact,
}
impl VertexEncoding {
    pub const fn stride(self) -> usize {
        match self {
            Self::Full => 32,
            Self::Wide => 28,
            Self::Compact => 20,
        }
    }
    #[cfg(any(windows, test))]
    pub(crate) fn scales(self) -> [f32; 4] {
        match self {
            Self::Full => [1.0, 1.0, 1.0, 0.0],
            Self::Wide => [1.0, 1.0, 1.0 / 32768.0, 0.0],
            Self::Compact => [1.0 / 128.0, 1.0 / 32768.0, 1.0 / 32768.0, 0.0],
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Compact {
    position: [i16; 4],
    color: u32,
    uv: [i16; 2],
    light: [i16; 2],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Wide {
    position: [f32; 3],
    color: u32,
    uv: [f32; 2],
    light: [i16; 2],
}
fn fixed(value: f32, scale: f32) -> Option<i16> {
    let n = value * scale;
    if !n.is_finite() || n < f32::from(i16::MIN) || n > f32::from(i16::MAX) {
        return None;
    }
    let integer = n as i16;
    ((f32::from(integer) / scale).to_bits() == value.to_bits()).then_some(integer)
}
fn pair(v: [f32; 2]) -> Option<[i16; 2]> {
    Some([fixed(v[0], 32768.0)?, fixed(v[1], 32768.0)?])
}
impl Compact {
    fn new(v: &Vertex) -> Option<Self> {
        Some(Self {
            position: [
                fixed(v.pos[0], 128.0)?,
                fixed(v.pos[1], 128.0)?,
                fixed(v.pos[2], 128.0)?,
                0,
            ],
            color: v.color,
            uv: pair(v.uv)?,
            light: pair(v.light)?,
        })
    }
}
impl Wide {
    fn new(v: &Vertex) -> Option<Self> {
        Some(Self {
            position: v.pos,
            color: v.color,
            uv: v.uv,
            light: pair(v.light)?,
        })
    }
}
/// Owned, validated vertex bytes. Only this module constructs their encoding and
/// count, so a worker can prepare them without handing COM objects across threads.
pub struct EncodedVertices {
    bytes: Vec<u8>,
    encoding: VertexEncoding,
    count: usize,
}
impl EncodedVertices {
    pub fn empty(encoding: VertexEncoding) -> Self {
        Self {
            bytes: Vec::new(),
            encoding,
            count: 0,
        }
    }
    /// Join complete quad ranges without decoding or changing their layout.
    pub fn append_range(
        &mut self,
        source: &Self,
        range: std::ops::Range<usize>,
    ) -> Result<(), String> {
        if self.encoding != source.encoding
            || range.start > range.end
            || range.end > source.count
            || range.start % 4 != 0
            || range.end % 4 != 0
        {
            return Err("D3D9 incompatible encoded quad range".into());
        }
        let count = self
            .count
            .checked_add(range.len())
            .filter(|&n| n <= u32::MAX as usize)
            .ok_or("D3D9 combined vertex count overflow")?;
        let stride = self.encoding.stride();
        self.bytes
            .extend_from_slice(&source.bytes[range.start * stride..range.end * stride]);
        self.count = count;
        Ok(())
    }
    pub fn new(vertices: &[Vertex], hardware_compact: bool) -> Self {
        let mut bytes = Vec::new();
        let mut encoding = VertexEncoding::Full;
        if hardware_compact {
            bytes.reserve(vertices.len() * size_of::<Compact>());
            if vertices.iter().all(|v| {
                if let Some(v) = Compact::new(v) {
                    bytes.extend_from_slice(bytemuck::bytes_of(&v));
                    true
                } else {
                    false
                }
            }) {
                encoding = VertexEncoding::Compact;
            } else {
                bytes.clear();
                if vertices.iter().all(|v| {
                    if let Some(v) = Wide::new(v) {
                        bytes.extend_from_slice(bytemuck::bytes_of(&v));
                        true
                    } else {
                        false
                    }
                }) {
                    encoding = VertexEncoding::Wide;
                }
            }
        }
        if encoding == VertexEncoding::Full {
            bytes.clear();
            bytes.extend_from_slice(bytemuck::cast_slice(vertices));
        }
        Self {
            bytes,
            encoding,
            count: vertices.len(),
        }
    }
    pub fn encoding(&self) -> VertexEncoding {
        self.encoding
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
pub(crate) fn expand_in_place(encoding: VertexEncoding, out: &mut [Vertex]) {
    if encoding == VertexEncoding::Full {
        return;
    }
    // Expand backwards: every destination is at or beyond its packed source.
    // No second decoded mesh allocation is needed during device recovery.
    for i in (0..out.len()).rev() {
        let at = i * encoding.stride();
        let bytes = &bytemuck::cast_slice::<_, u8>(out)[at..at + encoding.stride()];
        let vertex = match encoding {
            VertexEncoding::Compact => {
                let v: Compact = bytemuck::pod_read_unaligned(bytes);
                Vertex {
                    pos: [v.position[0], v.position[1], v.position[2]]
                        .map(|n| f32::from(n) / 128.0),
                    color: v.color,
                    uv: v.uv.map(|n| f32::from(n) / 32768.0),
                    light: v.light.map(|n| f32::from(n) / 32768.0),
                }
            }
            VertexEncoding::Wide => {
                let v: Wide = bytemuck::pod_read_unaligned(bytes);
                Vertex {
                    pos: v.position,
                    color: v.color,
                    uv: v.uv,
                    light: v.light.map(|n| f32::from(n) / 32768.0),
                }
            }
            _ => unreachable!(),
        };
        out[i] = vertex;
    }
}

/// Shader constants are explicit dot-product rows, independent of HLSL matrix
/// packing rules and D3D's fixed-function row-vector convention.
#[cfg(any(windows, test))]
pub(crate) fn constants(
    world: &[f32; 16],
    view: &[f32; 16],
    projection: &[f32; 16],
) -> [[f32; 4]; 8] {
    let world = glam::Mat4::from_cols_array(world);
    let mvp = glam::Mat4::from_cols_array(projection) * glam::Mat4::from_cols_array(view) * world;
    let mut out = [[0.0; 4]; 8];
    out[..4].copy_from_slice(&mvp.transpose().to_cols_array_2d());
    out[4..7].copy_from_slice(&world.transpose().to_cols_array_2d()[..3]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decode(encoded: &EncodedVertices) -> Vec<Vertex> {
        encoded
            .bytes
            .chunks_exact(encoded.encoding.stride())
            .map(|b| match encoded.encoding {
                VertexEncoding::Full => bytemuck::pod_read_unaligned(b),
                VertexEncoding::Wide => {
                    let v: Wide = bytemuck::pod_read_unaligned(b);
                    Vertex {
                        pos: v.position,
                        color: v.color,
                        uv: v.uv,
                        light: v.light.map(|n| f32::from(n) / 32768.0),
                    }
                }
                VertexEncoding::Compact => {
                    let v: Compact = bytemuck::pod_read_unaligned(b);
                    Vertex {
                        pos: [v.position[0], v.position[1], v.position[2]]
                            .map(|n| f32::from(n) / 128.0),
                        color: v.color,
                        uv: v.uv.map(|n| f32::from(n) / 32768.0),
                        light: v.light.map(|n| f32::from(n) / 32768.0),
                    }
                }
            })
            .collect()
    }
    #[test]
    fn smaller_encodings_preserve_vertices_exactly_and_fall_back_for_model_precision() {
        let base = Vertex {
            pos: [16.125, 64.5, -0.125],
            color: 0x8040ff12,
            uv: [0.125, 0.96875],
            light: [0.03125, 0.96875],
        };
        for (v, encoding, stride) in [
            (base, VertexEncoding::Compact, 20),
            (
                Vertex {
                    pos: [0.1, -0.0, f32::from_bits(0x7fc01234)],
                    uv: [1.0, 0.123456],
                    ..base
                },
                VertexEncoding::Wide,
                28,
            ),
            (
                Vertex {
                    light: [-0.0, f32::NAN],
                    ..base
                },
                VertexEncoding::Full,
                32,
            ),
        ] {
            for count in [4, 17, 33] {
                let input = vec![v; count];
                let encoded = EncodedVertices::new(&input, true);
                assert_eq!(encoded.encoding(), encoding);
                assert_eq!(encoded.bytes().len(), count * stride);
                assert_eq!(
                    bytemuck::cast_slice::<_, u8>(&decode(&encoded)),
                    bytemuck::cast_slice::<_, u8>(&input)
                );
                let mut scratch = Vec::new();
                let shadow = crate::MeshData::from_encoded(&encoded, &mut scratch);
                let mut restored = Vec::new();
                shadow.decode(&mut restored).unwrap();
                assert_eq!(
                    bytemuck::cast_slice::<_, u8>(&restored),
                    bytemuck::cast_slice::<_, u8>(&input)
                );
                let ffp = EncodedVertices::new(&input, false);
                assert_eq!(ffp.encoding(), VertexEncoding::Full);
                assert_eq!(ffp.bytes(), bytemuck::cast_slice::<_, u8>(&input));
            }
        }
    }
    #[test]
    fn sdk_vs2_reconstructs_clip_world_uv_and_light_for_both_declarations() {
        let base = Vertex {
            pos: [16.125, 64.5, -0.125],
            color: 0xff408012,
            uv: [0.125, 0.96875],
            light: [0.03125, 0.96875],
        };
        let world = glam::Mat4::from_translation(glam::Vec3::new(-12.0, -64.0, -10.0));
        let view = glam::Mat4::from_rotation_y(0.2);
        let projection = glam::Mat4::perspective_infinite_reverse_rh(1.1, 16.0 / 9.0, 0.05);
        for v in [
            base,
            Vertex {
                pos: [16.1, 64.5, -0.125],
                uv: [0.123456, 0.96875],
                ..base
            },
        ] {
            let encoded = EncodedVertices::new(&[v], true);
            let mut regs = [[[0.0; 4]; 16]; 12];
            let mut c = constants(
                &world.to_cols_array(),
                &view.to_cols_array(),
                &projection.to_cols_array(),
            );
            c[7] = encoded.encoding.scales();
            regs[2][..8].copy_from_slice(&c);
            match encoded.encoding {
                VertexEncoding::Compact => {
                    let p: Compact = bytemuck::pod_read_unaligned(&encoded.bytes);
                    regs[1][0] = p.position.map(f32::from);
                    regs[1][2][..2].copy_from_slice(&p.uv.map(f32::from));
                    regs[1][3][..2].copy_from_slice(&p.light.map(f32::from));
                }
                VertexEncoding::Wide => {
                    let p: Wide = bytemuck::pod_read_unaligned(&encoded.bytes);
                    regs[1][0] = [p.position[0], p.position[1], p.position[2], 1.0];
                    regs[1][2][..2].copy_from_slice(&p.uv);
                    regs[1][3][..2].copy_from_slice(&p.light.map(f32::from));
                }
                _ => panic!("expected hardware encoding"),
            }
            regs[1][1] = [0.25, 0.5, 0.75, 1.0];
            let kind = |t: u32| (((t >> 28) & 7) | ((t >> 8) & 24)) as usize;
            let mut at = 1;
            assert_eq!(program::WORDS[0], 0xfffe0200);
            while program::WORDS[at] != 0xffff {
                let instruction = program::WORDS[at];
                let op = instruction & 0xffff;
                let n = ((instruction >> 24) & 15) as usize;
                let args = &program::WORDS[at + 1..at + 1 + n];
                at += 1 + n;
                if op == 31 {
                    continue;
                }
                let read = |t: u32| {
                    let r = regs[kind(t)][(t & 0x7ff) as usize];
                    std::array::from_fn::<_, 4, _>(|i| {
                        let v = r[((t >> (16 + i * 2)) & 3) as usize];
                        if (t >> 24) & 15 == 1 { -v } else { v }
                    })
                };
                let a = args.get(1).map_or([0.0; 4], |&t| read(t));
                let b = args.get(2).map_or([0.0; 4], |&t| read(t));
                let value = match op {
                    81 => std::array::from_fn(|i| f32::from_bits(args[i + 1])),
                    1 => a,
                    5 => std::array::from_fn(|i| a[i] * b[i]),
                    9 => [a.iter().zip(b).map(|(x, y)| x * y).sum(); 4],
                    _ => panic!("unexpected VS2 opcode {op}"),
                };
                let out = &mut regs[kind(args[0])][(args[0] & 0x7ff) as usize];
                for i in 0..4 {
                    if (args[0] >> (16 + i)) & 1 != 0 {
                        out[i] = value[i];
                    }
                }
            }
            let expected_clip =
                (projection * view * world) * glam::Vec3::from_array(v.pos).extend(1.0);
            let expected_world = world.transform_point3(glam::Vec3::from_array(v.pos));
            for (got, wanted) in regs[4][0].iter().zip(expected_clip.to_array()) {
                assert!((got - wanted).abs() < 0.0001);
            }
            for (got, wanted) in regs[6][2][..3].iter().zip(expected_world.to_array()) {
                assert!((got - wanted).abs() < 0.0001);
            }
            assert_eq!(&regs[6][0][..2], &v.uv);
            assert_eq!(&regs[6][1][..2], &v.light);
            assert_eq!(regs[5][0], regs[1][1]);
        }
    }
}
