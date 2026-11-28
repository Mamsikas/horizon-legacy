//! Reuse solid draw plans when actual visible sections and facing ranges agree.
//! Camera position/rotation alone are not a change to immutable geometry.
use super::*;

const ALL_BUCKETS: u8 = (1 << 7) - 1;

#[derive(Clone, Copy)]
pub(super) struct Clip<'a> {
    pub frustum: &'a Frustum,
    pub camera: DVec3,
    pub layout: ColumnLayout,
    pub clusters: bool,
}

#[derive(PartialEq, Eq)]
pub(super) struct Trim {
    slot: u32,
    pub packet: usize,
    pub vertices: Range<u32>,
}

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
    saved_trims: Vec<Trim>,
    probe_trims: Vec<Trim>,
    trimmed_vertices: u64,
    generation: Option<(u64, u64)>,
}
impl SolidCache {
    pub fn bytes(&self) -> usize {
        (self.saved.capacity() + self.probe.capacity()) * size_of::<Stamp>()
            + (self.saved_trims.capacity() + self.probe_trims.capacity()) * size_of::<Trim>()
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
    pub fn trimmed_vertices(&self) -> u64 {
        self.trimmed_vertices
    }
    pub fn packet_trims(&self, slot: u32) -> &[Trim] {
        if self.generation.is_none() {
            return &[];
        }
        let start = self.saved_trims.partition_point(|trim| trim.slot < slot);
        let end = self.saved_trims[start..].partition_point(|trim| trim.slot == slot);
        &self.saved_trims[start..start + end]
    }

    pub fn update(
        &mut self,
        sections: &resident::Sections,
        visible: &[gfx::resident::Visible],
        generation: (u64, u64),
        sides: bool,
        fog: Option<dx::Fog>,
        clip: Option<Clip<'_>>,
    ) -> bool {
        self.probe.clear();
        self.probe_trims.clear();
        self.trimmed_vertices = 0;
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
                self.trimmed_vertices = 0;
                self.generation = None;
                return false;
            }
            let clipper = clip.and_then(|clip| {
                let bounds = section.solid_bounds.as_deref()?;
                let origin = Vec3::from_array(v.origin);
                if clip.frustum.contains(origin, origin + Vec3::splat(16.0)) {
                    return None;
                }
                let region = mesh::region_origin(mesh::region_key(v.key), clip.layout, clip.camera);
                Some((Clipper::new(clip.frustum, region), bounds, clip.clusters))
            });
            let buckets = clipper
                .as_ref()
                .map_or(ALL_BUCKETS, |(clipper, bounds, _)| {
                    visible_buckets(bounds, clipper)
                });
            if clip.is_some() {
                self.visible_buckets.push(buckets);
            }
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
            if let Some((clipper, _, true)) = clipper
                && !section.solid_clusters.is_empty()
            {
                for (i, packet) in section.solid.iter().enumerate() {
                    if packet.clusters.is_empty()
                        || buckets & (1 << packet.bucket) == 0
                        || !stamp.ranges[..stamp.count].iter().any(|&[first, count]| {
                            first < packet.source.end && packet.source.start < first + count
                        })
                    {
                        continue;
                    }
                    let clusters = &section.solid_clusters
                        [usize::from(packet.clusters.start)..usize::from(packet.clusters.end)];
                    let range = cluster_window(clusters, packet.count, &clipper);
                    if range.len() != packet.count as usize {
                        self.trimmed_vertices += (packet.count as usize - range.len()) as u64;
                        self.probe_trims.push(Trim {
                            slot: v.slot,
                            packet: i,
                            vertices: packet.first + range.start..packet.first + range.end,
                        });
                    }
                }
            }
            self.probe.push(stamp);
        }
        // Resident slots identify data, not distance. No lossy fingerprint is
        // allowed to hide a changed section, camera-facing range or reset.
        self.probe.sort_unstable_by_key(|s| s.slot);
        self.probe_trims
            .sort_unstable_by_key(|trim| (trim.slot, trim.packet));
        if self.generation == Some(generation)
            && self.saved == self.probe
            && self.saved_trims == self.probe_trims
        {
            true
        } else {
            std::mem::swap(&mut self.saved, &mut self.probe);
            std::mem::swap(&mut self.saved_trims, &mut self.probe_trims);
            self.generation = Some(generation);
            false
        }
    }
}

struct Clipper<'a> {
    frustum: &'a Frustum,
    min_offset: Vec3,
    max_offset: Vec3,
}

impl<'a> Clipper<'a> {
    fn new(frustum: &'a Frustum, origin: Vec3) -> Self {
        // Round away from the clip planes to cover the f32 region transform
        // and driver matrix arithmetic.
        let pad =
            Vec3::splat(1.0 / 64.0 + 16.0 * f32::EPSILON * (origin.abs().max_element() + 128.0));
        Self {
            frustum,
            min_offset: origin - pad,
            max_offset: origin + pad,
        }
    }
    fn outside(&self, [lo, hi]: mesh::Bounds) -> bool {
        lo.is_finite()
            && hi.is_finite()
            && lo.cmple(hi).all()
            && !self
                .frustum
                .intersects(lo + self.min_offset, hi + self.max_offset)
    }
}

fn visible_buckets(bounds: &mesh::SolidBounds, clipper: &Clipper<'_>) -> u8 {
    let mut keep = ALL_BUCKETS;
    for (i, &bounds) in bounds.iter().enumerate() {
        if clipper.outside(bounds) {
            keep &= !(1 << i);
        }
    }
    keep
}

fn cluster_window(clusters: &[mesh::Bounds], count: u32, clipper: &Clipper<'_>) -> Range<u32> {
    let Some(first) = clusters.iter().position(|&bounds| !clipper.outside(bounds)) else {
        return 0..0;
    };
    // Only trim the two ends. A hole inside the window would add a draw
    // and trade the saved SWVP vertices for a new driver submission.
    let last = first
        + clusters[first..]
            .iter()
            .rposition(|&bounds| !clipper.outside(bounds))
            .unwrap_or(0);
    let width = mesh::cluster_vertices(count);
    first as u32 * width..((last + 1) as u32 * width).min(count)
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
            visible_buckets(&bounds, &Clipper::new(&frustum, Vec3::ZERO)),
            ALL_BUCKETS & !1
        );
        assert_eq!(
            visible_buckets(&bounds, &Clipper::new(&frustum, Vec3::NEG_X)),
            ALL_BUCKETS
        );
    }

    #[test]
    fn cluster_clipping_trims_only_ends_and_keeps_unknown_bounds() {
        let frustum = Frustum::boxed(Vec3::splat(-1.0), Vec3::ONE);
        let clipper = Clipper::new(&frustum, Vec3::ZERO);
        let outside = [Vec3::splat(2.0), Vec3::splat(3.0)];
        let inside = [Vec3::ZERO, Vec3::ONE];
        let unknown = [Vec3::splat(f32::NAN); 2];
        assert_eq!(
            cluster_window(
                &[outside, inside, outside, unknown, outside],
                1028,
                &clipper
            ),
            256..1024
        );
        assert_eq!(cluster_window(&[outside], 4, &clipper), 0..0);
        assert_eq!(cluster_window(&[unknown], 4, &clipper), 0..4);
    }
}
