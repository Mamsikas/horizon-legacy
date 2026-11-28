//! Balanced intervals for the dev SWVP standing suite.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trial {
    DenseIndices,
    SamplerReuse,
    GlintStaging,
    BaseTextureReuse,
    SolidClusters,
}

impl Trial {
    pub(super) fn enabled(self, software_vertices: bool, slot: Option<Slot>) -> bool {
        software_vertices
            && slot.map_or_else(
                || matches!(self, Self::SolidClusters | Self::BaseTextureReuse),
                |slot| slot.active(self),
            )
    }
}

// Each arm appears once in every position, and every ordered pair of
// different arms occurs once within a block.
const ORDER: [[u8; 4]; 4] = [[0, 1, 3, 2], [1, 2, 0, 3], [2, 3, 1, 0], [3, 0, 2, 1]];
const ENDS: [u32; 4] = [38, 75, 113, 150];
const BLOCK_SECONDS: u32 = ENDS[3];
const WARMUP_SECONDS: u32 = 2;
pub const SECONDS: u32 = BLOCK_SECONDS * ORDER.len() as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub number: u32,
    arm: u8,
    pub measuring: bool,
}

impl Slot {
    pub fn at(elapsed_seconds: u32) -> Option<Self> {
        let block = elapsed_seconds / BLOCK_SECONDS;
        let order = ORDER.get(block as usize)?;
        let within = elapsed_seconds % BLOCK_SECONDS;
        let position = ENDS.partition_point(|&end| end <= within);
        let start = if position == 0 { 0 } else { ENDS[position - 1] };
        Some(Self {
            number: block * 4 + position as u32,
            arm: order[position],
            measuring: within - start >= WARMUP_SECONDS,
        })
    }

    pub fn active(self, trial: Trial) -> bool {
        match trial {
            Trial::SolidClusters => self.arm & 1 != 0,
            Trial::BaseTextureReuse => self.arm & 2 != 0,
            Trial::DenseIndices | Trial::SamplerReuse | Trial::GlintStaging => false,
        }
    }

    pub(super) fn arm(self) -> char {
        char::from(b'A' + self.arm)
    }

    pub fn marker(self) -> String {
        format!(
            "DX9 benchmark slot={} trial=Terrain arm={} measuring={}",
            self.number,
            self.arm(),
            self.measuring
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_measurement_time_and_balanced_order_for_the_four_arms() {
        assert_eq!(SECONDS, 600);
        let mut measured = [0; 4];
        for second in 0..SECONDS {
            let slot = Slot::at(second).unwrap();
            if slot.measuring {
                measured[usize::from(slot.arm)] += 1;
            }
            assert_eq!(
                Trial::SolidClusters.enabled(true, Some(slot)),
                matches!(slot.arm, 1 | 3)
            );
            assert_eq!(
                Trial::BaseTextureReuse.enabled(true, Some(slot)),
                matches!(slot.arm, 2 | 3)
            );
        }
        assert_eq!(measured, [142; 4]);
        let mut transitions = [[0; 4]; 4];
        for order in ORDER {
            for pair in order.windows(2) {
                transitions[usize::from(pair[0])][usize::from(pair[1])] += 1;
            }
        }
        for (from, moves) in transitions.iter().enumerate() {
            for (to, &count) in moves.iter().enumerate() {
                assert_eq!(count, u8::from(from != to));
            }
        }
        assert!(Slot::at(SECONDS).is_none());
    }

    #[test]
    fn unlabelled_software_vertex_draws_use_both_measured_optimizations() {
        assert!(Trial::SolidClusters.enabled(true, None));
        assert!(Trial::BaseTextureReuse.enabled(true, None));
        for trial in [
            Trial::DenseIndices,
            Trial::SamplerReuse,
            Trial::GlintStaging,
        ] {
            assert!(!trial.enabled(true, None));
        }
        for trial in [Trial::SolidClusters, Trial::BaseTextureReuse] {
            assert!(!trial.enabled(false, None));
            assert!(!trial.enabled(false, Slot::at(75)));
        }
    }
}
