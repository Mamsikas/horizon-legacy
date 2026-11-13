//! Reuse solid draw plans when actual visible sections and facing ranges agree.
//! Camera position/rotation alone are not a change to immutable geometry.
use super::*;

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    slot: u32,
    revision: u64,
    ranges: [[u32; 2]; 14],
    count: usize,
}
#[derive(Default)]
pub(super) struct SolidCache {
    saved: Vec<Stamp>,
    probe: Vec<Stamp>,
    generation: Option<(u64, u64)>,
}
impl SolidCache {
    pub fn bytes(&self) -> usize {
        (self.saved.capacity() + self.probe.capacity()) * size_of::<Stamp>()
    }
    pub fn update(
        &mut self,
        sections: &resident::Sections,
        visible: &[gfx::resident::Visible],
        generation: (u64, u64),
        sides: bool,
        hardware: bool,
    ) -> bool {
        self.probe.clear();
        if !hardware {
            self.generation = None;
            return false;
        }
        for v in visible {
            let section = sections.at(v.slot);
            if section.solid.is_empty() {
                continue;
            }
            if section.gpu.is_empty() {
                self.generation = None;
                return false;
            }
            let mut stamp = Stamp {
                slot: v.slot,
                revision: section.revision,
                ranges: [[0; 2]; 14],
                count: 0,
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
