//! Already projected HUD vertices bypass D3D9's software transform/lighting.
use crate::Vertex;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScreenVertex {
    pub position: [f32; 4],
    pub color: u32,
    pub uv: [f32; 2],
}
impl ScreenVertex {
    /// Same orthographic viewport and half-pixel correction as the ordinary
    /// HUD projection, including depth for inventory models and scissor clips.
    pub fn from_gui(vertex: &Vertex, scale: f32) -> Self {
        Self {
            position: [
                vertex.pos[0] * scale - 0.5,
                vertex.pos[1] * scale - 0.5,
                vertex.pos[2],
                1.0,
            ],
            color: vertex.color,
            uv: vertex.uv,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn screen_vertices_keep_subpixel_positions_depth_and_attributes() {
        let vertex = Vertex {
            pos: [7.25, -3.5, 0.125],
            color: 0x7f123456,
            uv: [0.125, 0.875],
            light: [0.0; 2],
        };
        let screen = ScreenVertex::from_gui(&vertex, 2.0);
        assert_eq!(screen.position, [14.0, -7.5, 0.125, 1.0]);
        assert_eq!(screen.color, 0x7f123456);
        assert_eq!(screen.uv, [0.125, 0.875]);
        assert_eq!(ScreenVertex::from_gui(&vertex, 1.5).position[0], 10.375);
    }
}
