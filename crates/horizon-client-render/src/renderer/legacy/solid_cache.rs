//! Reuse solid draw plans when actual visible sections and facing ranges agree.
//! Camera position/rotation alone are not a change to immutable geometry.
use super::*;

const ALL_BUCKETS: u8 = (1 << 7) - 1;

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    slot: u32,
    revision: u64,
    ranges: [[u32; 2]; 14],
    count: usize,
    fog: dx::FogProgram,
    buckets: u8,
}
#[derive(Default)]
pub(super) struct SolidCache {
    saved: Vec<Stamp>,
    probe: Vec<Stamp>,
    visible_buckets: Vec<u8>,
    generation: Option<(u64, u64)>,
}
impl SolidCache {
    pub fn bytes(&self) -> usize {
        (self.saved.capacity() + self.probe.capacity()) * size_of::<Stamp>()
            + self.visible_buckets.capacity()
    }
    pub fn buckets(&self, visible_index: usize) -> u8 {
        self.visible_buckets
            .get(visible_index)
            .copied()
            .unwrap_or(ALL_BUCKETS)
    }
    pub fn culled_buckets(&self) -> u32 {
        self.visible_buckets
            .iter()
            .map(|mask| (ALL_BUCKETS ^ mask).count_ones())
            .sum()
    }
    pub fn update(
        &mut self,
        sections: &resident::Sections,
        visible: &[gfx::resident::Visible],
        generation: (u64, u64),
        sides: bool,
        fog: Option<dx::Fog>,
        clip: Option<(&Frustum, DVec3, ColumnLayout)>,
    ) -> bool {
        self.probe.clear();
        self.visible_buckets.clear();
        for v in visible {
            let section = sections.at(v.slot);
            if section.solid.is_empty() {
                if clip.is_some() {
                    self.visible_buckets.push(ALL_BUCKETS);
                }
                continue;
            }
            if section.gpu.is_empty() {
                // Cold frames keep their original draws. The plan next frame
                // must include all sections before any saved mask can be used.
                self.visible_buckets.clear();
                self.generation = None;
                return false;
            }
            let buckets = if let Some((frustum, camera, layout)) = clip {
                let origin = Vec3::from_array(v.origin);
                let keep = section
                    .solid_bounds
                    .as_deref()
                    .filter(|_| !frustum.contains(origin, origin + Vec3::splat(16.0)))
                    .map_or(ALL_BUCKETS, |bounds| {
                        visible_buckets(
                            bounds,
                            frustum,
                            mesh::region_origin(mesh::region_key(v.key), layout, camera),
                        )
                    });
                self.visible_buckets.push(keep);
                keep
            } else {
                ALL_BUCKETS
            };
            let mut stamp = Stamp {
                slot: v.slot,
                revision: section.revision,
                ranges: [[0; 2]; 14],
                count: 0,
                fog: section_fog(sections, v, fog),
                buckets,
            };
            if sides {
                section
                    .sides
                    .visible((-Vec3::from_array(v.origin)).to_array(), |first, count| {
                        stamp.ranges[stamp.count] = [first, count];
                        stamp.count += 1;
                    });
            } else {
                stamp.ranges[0] = [0, u32::MAX];
                stamp.count = 1;
            }
            self.probe.push(stamp);
        }
        // Resident slots identify data, not distance. No lossy fingerprint is
        // allowed to hide a changed section, camera-facing range or reset.
        self.probe.sort_unstable_by_key(|s| s.slot);
        if self.generation == Some(generation) && self.saved == self.probe {
            true
        } else {
            std::mem::swap(&mut self.saved, &mut self.probe);
            self.generation = Some(generation);
            false
        }
    }
}

fn visible_buckets(bounds: &mesh::SolidBounds, frustum: &Frustum, origin: Vec3) -> u8 {
    // Round away from the clip planes to cover the f32 region transform and
    // driver matrix arithmetic. Unbounded/invalid geometry stays on the old path.
    let pad = Vec3::splat(1.0 / 64.0 + 16.0 * f32::EPSILON * (origin.abs().max_element() + 128.0));
    let mut keep = ALL_BUCKETS;
    for (i, &[lo, hi]) in bounds.iter().enumerate() {
        if lo.is_finite()
            && hi.is_finite()
            && lo.cmple(hi).all()
            && !frustum.intersects(lo + origin - pad, hi + origin + pad)
        {
            keep &= !(1 << i);
        }
    }
    keep
}

pub(super) fn section_fog(
    sections: &resident::Sections,
    visible: &gfx::resident::Visible,
    fog: Option<dx::Fog>,
) -> dx::FogProgram {
    match (fog, sections.bounds(visible.slot)) {
        (Some(fog), Some(bounds)) => {
            let origin = Vec3::from_array(visible.origin);
            fog.program(bounds.map(|b| (b + origin).to_array()))
        }
        _ => dx::FogProgram::Dual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_group_culling_keeps_clip_edges_and_unknown_bounds() {
        let frustum = Frustum::boxed(Vec3::splat(-1.0), Vec3::ONE);
        let mut bounds = [[Vec3::splat(f32::NAN); 2]; 7];
        bounds[0] = [Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 1.0)];
        bounds[1] = [Vec3::new(1.005, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0)];
        assert_eq!(
            visible_buckets(&bounds, &frustum, Vec3::ZERO),
            ALL_BUCKETS & !1
        );
        assert_eq!(visible_buckets(&bounds, &frustum, Vec3::NEG_X), ALL_BUCKETS);
    }
}
