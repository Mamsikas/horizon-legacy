//! The authored honey cube lists its outer shell before its inset cube.
//! With back-face culling, rays from outside meet the shell before the inset.
//! Put the inset first at mesh conversion so alpha composes back-to-front.
use super::{TerrainVertex, Vec3};

#[derive(Clone, Copy)]
pub(super) struct Layers(pub [u16; 3]);

impl Layers {
    fn surface(self, quad: &[TerrainVertex; 4]) -> Option<([i32; 3], bool)> {
        let layer = quad[0].layer & !crate::mesher::OPAQUE_LAYER;
        if !self.0.contains(&layer) {
            return None;
        }
        let pos = quad.map(|v| Vec3::from_array(v.pos));
        let lo = pos.iter().fold(pos[0], |a, b| a.min(*b));
        let hi = pos.iter().fold(pos[0], |a, b| a.max(*b));
        let axis = (0..3).find(|&i| lo[i] == hi[i])?;
        let normal = (pos[1] - pos[0]).cross(pos[2] - pos[0])[axis];
        if normal == 0.0 || !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        let base = lo.floor();
        let outer = layer == self.0[0] && lo[axis] == base[axis];
        let mut block = base.to_array().map(|v| v as i32);
        let inset = if outer { 0.0 } else { 1.0 / 16.0 };
        if outer {
            block[axis] -= i32::from(normal > 0.0);
        } else {
            let plane = if normal > 0.0 { 1.0 - inset } else { inset };
            if lo[axis] - base[axis] != plane {
                return None;
            }
        }
        (0..3)
            .filter(|&i| i != axis)
            .all(|i| lo[i] == base[i] + inset && hi[i] == base[i] + 1.0 - inset)
            .then_some((block, outer))
    }

    pub(super) fn in_draw_order<E>(
        self,
        mut quads: &[[TerrainVertex; 4]],
        mut emit: impl FnMut(&[TerrainVertex; 4]) -> Result<(), E>,
    ) -> Result<(), E> {
        while let Some((quad, rest)) = quads.split_first() {
            let Some((block, _)) = self.surface(quad) else {
                emit(quad)?;
                quads = rest;
                continue;
            };
            let count = 1 + rest
                .iter()
                .take_while(|q| self.surface(q).is_some_and(|(cell, _)| cell == block))
                .count();
            let (block_quads, rest) = quads.split_at(count);
            for outer in [false, true] {
                for q in block_quads {
                    if self.surface(q).is_some_and(|(_, shell)| shell == outer) {
                        emit(q)?;
                    }
                }
            }
            quads = rest;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_honey_quads_keep_the_inset_behind_the_shell_and_keep_neighbour_order() {
        let top = |x: f32, inset: f32, layer| {
            let lo = inset;
            let hi = 1.0 - inset;
            [[lo, lo], [lo, hi], [hi, hi], [hi, lo]].map(|[dx, z]| {
                TerrainVertex::new([x + dx, hi, z], [dx, z], layer, 0xff_ff_ff_ff, 0)
            })
        };
        let other = top(0.0, 0.0, 7);
        let outer = top(0.0, 0.0, 10);
        let core = top(0.0, 1.0 / 16.0, 12);
        let next = top(1.0, 0.0, 10);
        let mut order = Vec::new();
        Layers([10, 11, 12])
            .in_draw_order(&[outer, core, next, other], |quad| {
                order.push((quad[0].layer, quad[0].pos));
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(
            order,
            [core, outer, next, other].map(|q| (q[0].layer, q[0].pos))
        );
    }
}
