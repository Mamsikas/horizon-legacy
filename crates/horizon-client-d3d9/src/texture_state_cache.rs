//! Numeric D3D9 texture-stage and sampler values for one native device.
pub(crate) struct TextureStateCache {
    stages: [[Option<u32>; 33]; 3],
    samplers: [[Option<u32>; 14]; 3],
    presented: bool,
}

impl Default for TextureStateCache {
    fn default() -> Self {
        Self {
            stages: [[None; 33]; 3],
            samplers: [[None; 14]; 3],
            presented: false,
        }
    }
}

impl TextureStateCache {
    pub(crate) fn begin_frame(&mut self, retain_values: bool) {
        if !retain_values || !self.presented {
            self.invalidate();
        }
        self.presented = false;
    }

    pub(crate) fn presented(&mut self) {
        self.presented = true;
    }

    pub(crate) fn invalidate(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn set_stage<E>(
        &mut self,
        stage: u32,
        state: i32,
        value: u32,
        submit: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        set_known(
            &mut self.stages[stage as usize][state as usize],
            value,
            submit,
        )
    }

    pub(crate) fn set_sampler<E>(
        &mut self,
        sampler: u32,
        state: i32,
        value: u32,
        submit: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        set_known(
            &mut self.samplers[sampler as usize][state as usize],
            value,
            submit,
        )
    }
}

#[inline(always)]
fn set_known<E>(
    slot: &mut Option<u32>,
    value: u32,
    submit: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    if *slot != Some(value) {
        *slot = None;
        submit()?;
        *slot = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn texture_values_survive_present_and_refresh_after_invalidations() {
        let mut cache = TextureStateCache::default();
        let submitted = Cell::new(0);
        let submit = || {
            submitted.set(submitted.get() + 1);
            Ok::<(), ()>(())
        };

        cache.begin_frame(true);
        cache.set_stage(0, 1, 10, submit).unwrap();
        cache.set_sampler(1, 5, 20, submit).unwrap();
        assert_eq!(submitted.get(), 2);
        cache.presented();
        cache.begin_frame(true);
        cache.set_stage(0, 1, 10, submit).unwrap();
        cache.set_sampler(1, 5, 20, submit).unwrap();
        assert_eq!(submitted.get(), 2);

        // A failed setter, including a change followed by retry of the old value.
        assert_eq!(cache.set_stage(0, 1, 11, || Err(())), Err(()));
        cache.set_stage(0, 1, 10, submit).unwrap();
        assert_eq!(submitted.get(), 3);
        // An abandoned scene has no successful Present.
        cache.begin_frame(true);
        cache.set_stage(0, 1, 10, submit).unwrap();
        cache.set_sampler(1, 5, 20, submit).unwrap();
        assert_eq!(submitted.get(), 5);

        cache.presented();
        cache.invalidate(); // Reset, resize or renderer replacement.
        cache.begin_frame(true);
        cache.set_stage(0, 1, 10, submit).unwrap();
        cache.set_sampler(1, 5, 20, submit).unwrap();
        assert_eq!(submitted.get(), 7);
        cache.presented();
        cache.begin_frame(false);
        cache.set_stage(0, 1, 10, submit).unwrap();
        cache.set_sampler(1, 5, 20, submit).unwrap();
        assert_eq!(submitted.get(), 9);
    }
}
