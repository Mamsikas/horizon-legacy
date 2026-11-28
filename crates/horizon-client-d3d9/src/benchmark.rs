//! Balanced intervals for the dev SWVP standing suite.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trial {
    DenseIndices,
    FaceBounds,
    SamplerReuse,
    GlintStaging,
    BaseTextureReuse,
}

const TRIALS: [Trial; 5] = [
    Trial::DenseIndices,
    Trial::FaceBounds,
    Trial::SamplerReuse,
    Trial::GlintStaging,
    Trial::BaseTextureReuse,
];
const CANDIDATE: [bool; 8] = [false, true, true, false, true, false, false, true];
const SLOT_SECONDS: u32 = 15;
const WARMUP_SECONDS: u32 = 2;
pub const SECONDS: u32 = SLOT_SECONDS * CANDIDATE.len() as u32 * TRIALS.len() as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub number: u32,
    pub trial: Trial,
    pub candidate: bool,
    pub measuring: bool,
}

impl Slot {
    pub fn at(elapsed_seconds: u32) -> Option<Self> {
        let number = elapsed_seconds / SLOT_SECONDS;
        let index = number as usize;
        Some(Self {
            number,
            trial: *TRIALS.get(index / CANDIDATE.len())?,
            candidate: CANDIDATE[index % CANDIDATE.len()],
            measuring: elapsed_seconds % SLOT_SECONDS >= WARMUP_SECONDS,
        })
    }

    pub fn marker(self) -> String {
        let arm = if self.candidate { "B" } else { "A" };
        format!(
            "DX9 benchmark slot={} trial={:?} arm={arm} measuring={}",
            self.number, self.trial, self.measuring
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_the_measurement_time_in_each_arm_for_every_trial() {
        assert_eq!(SECONDS, 600);
        let mut slots = [[0; 2]; TRIALS.len()];
        for second in 0..SECONDS {
            let slot = Slot::at(second).unwrap();
            assert_eq!(slot.number, second / SLOT_SECONDS);
            assert_eq!(slot.measuring, second % SLOT_SECONDS >= 2);
            if slot.measuring {
                let i = TRIALS
                    .iter()
                    .position(|&trial| trial == slot.trial)
                    .unwrap();
                slots[i][usize::from(slot.candidate)] += 1;
            }
        }
        assert_eq!(slots, [[52, 52]; TRIALS.len()]);
        assert!(Slot::at(SECONDS).is_none());
    }
}
