//! Stream-0 state only: no COM reference or resource survives through this key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub buffer: usize,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Default)]
pub(crate) struct StreamCache(Option<Binding>);
impl StreamCache {
    pub fn invalidate(&mut self) {
        self.0 = None;
    }

    #[inline(always)]
    pub fn bind<E>(
        &mut self,
        binding: Binding,
        enabled: bool,
        submit: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        if enabled && self.0 == Some(binding) {
            return Ok(());
        }
        // A failed API call must never leave a trusted old binding behind.
        self.invalidate();
        submit()?;
        if enabled {
            self.0 = Some(binding);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn streams_layouts_failures_and_external_invalidation_keep_native_state_coherent() {
        let a = Binding {
            buffer: 1,
            offset: 0,
            stride: 32,
        };
        let other = Binding { buffer: 2, ..a };
        let screen = Binding {
            buffer: 2,
            offset: 0,
            stride: 28,
        };
        let offset = Binding { offset: 32, ..a };
        let compact = Binding {
            stride: 20,
            ..offset
        };
        let mut cache = StreamCache::default();
        let native = Cell::new(None);
        let sent = RefCell::new(Vec::new());
        for binding in [a, a, other, other, a, offset, compact, screen, screen, a] {
            cache
                .bind(binding, true, || {
                    native.set(Some(binding));
                    sent.borrow_mut().push(binding);
                    Ok::<_, ()>(())
                })
                .unwrap();
            assert_eq!(native.get(), Some(binding));
        }
        assert_eq!(*sent.borrow(), [a, other, a, offset, compact, screen, a]);
        assert!(
            cache
                .bind(screen, true, || {
                    native.set(None);
                    Err::<(), _>(())
                })
                .is_err()
        );
        cache
            .bind(a, true, || {
                native.set(Some(a));
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(native.get(), Some(a));
        // Reset/frame/DISCARD barriers require an actual bind, even for the
        // same COM identity. Disabled (hardware/normal) mode always submits.
        cache.invalidate();
        for enabled in [true, false, false] {
            native.set(None);
            cache
                .bind(a, enabled, || {
                    native.set(Some(a));
                    Ok::<_, ()>(())
                })
                .unwrap();
            assert_eq!(native.get(), Some(a));
        }
    }
}
