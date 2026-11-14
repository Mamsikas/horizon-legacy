//! Already projected HUD vertices bypass D3D9's software transform/lighting.
use crate::{PaintWindow, Vertex};

/// Only a depth-writing model intersecting previous model pixels needs another
/// clear. Independent inventory slots can share depth and adjacent draw ranges.
#[derive(Default)]
pub struct GuiDepth {
    window: PaintWindow,
    models: usize,
    pub clears: u64,
    pub reused: u64,
}
impl GuiDepth {
    pub fn begin(&mut self) {
        self.window.clear();
        self.models = 0;
        self.clears = 0;
        self.reused = 0;
    }
    pub fn bytes(&self) -> usize {
        self.window.bytes()
    }
    pub fn clear_for(&mut self, positions: impl IntoIterator<Item = [f32; 2]>, scale: f32) -> bool {
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut finite = scale.is_finite() && scale > 0.0;
        for pos in positions {
            for axis in 0..2 {
                let p = pos[axis] * scale;
                finite &= p.is_finite();
                bounds[axis] = bounds[axis].min(p);
                bounds[axis + 2] = bounds[axis + 2].max(p);
            }
        }
        // D3D9's -0.5 placement and integer sample centres fit in this
        // conservative half-open pixel rectangle, including fractional scales.
        let rect = if finite && bounds.iter().all(|v| v.is_finite()) {
            [
                bounds[0].floor(),
                bounds[1].floor(),
                bounds[2].ceil(),
                bounds[3].ceil(),
            ]
        } else {
            [
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::INFINITY,
            ]
        };
        // Unique ids prevent PaintWindow from combining unrelated model bounds.
        // Its bounded window returns a barrier on overlap or capacity exhaustion.
        let overlap = self.window.barrier(self.models, rect);
        let clear = self.models == 0 || overlap;
        self.models += 1;
        if clear {
            self.clears += 1;
        } else {
            self.reused += 1;
        }
        clear
    }
}

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
    #[test]
    fn independent_inventory_models_share_depth_without_changing_the_image() {
        // Each model draws its near face then a far face: clearing inside a
        // model or keeping stale depth in an overlapping slot changes colour.
        let models = [
            [0, 0, 3, 3],
            [5, 0, 8, 3],
            [0, 5, 3, 8],
            [1, 1, 4, 4],
            [5, 5, 8, 8],
        ];
        let render = |reuse: bool| {
            let mut depth = vec![1.0f32; 100];
            let mut color = vec![0u8; 100];
            let mut coverage = GuiDepth::default();
            coverage.begin();
            for (id, [x0, y0, x1, y1]) in models.into_iter().enumerate() {
                let positions = [[x0 as f32, y0 as f32], [x1 as f32, y1 as f32]];
                let clear = coverage.clear_for(positions, 1.0);
                if !reuse || clear {
                    depth.fill(1.0);
                }
                for (z, ink) in [(0.25, (id + 1) as u8), (0.75, 99)] {
                    for y in y0..y1 {
                        for x in x0..x1 {
                            let at = (y * 10 + x) as usize;
                            if z <= depth[at] {
                                depth[at] = z;
                                color[at] = ink;
                            }
                        }
                    }
                }
            }
            (color, coverage.clears, coverage.reused)
        };
        let reference = render(false);
        let optimized = render(true);
        assert_eq!(optimized.0, reference.0);
        assert_eq!((optimized.1, optimized.2), (2, 3));
        let mut coverage = GuiDepth::default();
        coverage.begin();
        assert!(coverage.clear_for([[0.25, 0.25], [3.75, 3.75]], 1.5));
        assert!(!coverage.clear_for([[5.25, 0.25], [8.75, 3.75]], 1.5));
        assert!(coverage.clear_for([[3.5, 0.5], [4.0, 1.0]], 1.5));
        assert!(coverage.clear_for([[f32::NAN, 0.0]], 1.0));
        coverage.begin();
        assert!(coverage.clear_for([[50.0, 50.0], [51.0, 51.0]], 1.0));
        for i in 1..128 {
            assert!(!coverage.clear_for(
                [[i as f32 * 100.0, 0.0], [i as f32 * 100.0 + 1.0, 1.0]],
                1.0
            ));
        }
        assert!(coverage.clear_for([[20000.0, 0.0], [20001.0, 1.0]], 1.0));
    }
}
