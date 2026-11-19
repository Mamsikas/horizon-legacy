//! Bounded submission accounting. No draw clocks, queries, allocation or strings
//! in the hot path. The entire collector is absent without `submit-profile`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum SubmitPass {
    #[default]
    Outside,
    Begin,
    Prepare,
    Solid,
    DynamicEntities,
    StaticEntities,
    Translucent,
    Effects,
    Gui,
    Present,
}

#[cfg(all(windows, feature = "submit-profile"))]
pub(crate) use enabled::*;

#[cfg(feature = "submit-profile")]
mod enabled {
    #![cfg_attr(not(windows), allow(dead_code))]
    use super::SubmitPass;
    use std::{cell::RefCell, time::Instant};

    pub(crate) const PASSES: [SubmitPass; 10] = [
        SubmitPass::Outside,
        SubmitPass::Begin,
        SubmitPass::Prepare,
        SubmitPass::Solid,
        SubmitPass::DynamicEntities,
        SubmitPass::StaticEntities,
        SubmitPass::Translucent,
        SubmitPass::Effects,
        SubmitPass::Gui,
        SubmitPass::Present,
    ];

    #[derive(Clone, Copy)]
    #[repr(usize)]
    pub(crate) enum Api {
        DrawIndexedPrimitive,
        DrawPrimitive,
        SetStreamSource,
        SetIndices,
        SetTexture,
        SetTransform,
        SetVertexShader,
        SetPixelShader,
        SetRenderState,
        SetSamplerState,
        SetTextureStageState,
        LockVertex,
        UnlockVertex,
        LockIndex,
        UnlockIndex,
        CreateVertexBuffer,
        CreateIndexBuffer,
        LockTexture,
        UnlockTexture,
        Clear,
    }
    pub(crate) const API_COUNT: usize = Api::Clear as usize + 1;
    pub(crate) const API_NAMES: &str = "DrawIndexedPrimitive,DrawPrimitive,SetStreamSource,SetIndices,SetTexture,SetTransform,SetVertexShader,SetPixelShader,SetRenderState,SetSamplerState,SetTextureStageState,LockVertex,UnlockVertex,LockIndex,UnlockIndex,CreateVertexBuffer,CreateIndexBuffer,LockTexture,UnlockTexture,Clear";

    /// Power-of-two upper bounds (1, 2, 4, ... 65536, ... 2^32); zero shares bin 0.
    /// Quantiles are bucket bounds, never advertised as exact percentiles.
    #[derive(Debug)]
    pub(crate) struct Ranges {
        bins: [u64; 33],
        pub count: u64,
        pub sum: u64,
        pub min: Option<u32>,
        pub max: u32,
    }
    impl Default for Ranges {
        fn default() -> Self {
            Self {
                bins: [0; 33],
                count: 0,
                sum: 0,
                min: None,
                max: 0,
            }
        }
    }
    impl Ranges {
        fn add(&mut self, value: u32) {
            let bin = if value == 0 {
                0
            } else {
                (u32::BITS - (value - 1).leading_zeros()) as usize
            };
            self.bins[bin] += 1;
            self.count += 1;
            self.sum += u64::from(value);
            self.min = Some(self.min.map_or(value, |v| v.min(value)));
            self.max = self.max.max(value);
        }
        pub fn percentile_upper(&self, percent: u64) -> Option<u64> {
            if self.count == 0 {
                return None;
            }
            let rank = (self.count / 100 * percent) + (self.count % 100 * percent).div_ceil(100);
            let mut count = 0;
            for (bin, &n) in self.bins.iter().enumerate() {
                count += n;
                if count >= rank {
                    return Some(1u64 << bin);
                }
            }
            None
        }
    }

    #[derive(Default)]
    pub(crate) struct Counters {
        pub calls: [u64; API_COUNT],
        pub failures: [u64; API_COUNT],
        /// Requests reaching the low-level cache, after DrawState's whole-key cache.
        pub cache_requests: [u64; 3],
        pub state_requests: u64,
        pub transform_requests: u64,
        pub indexed_vertices: Ranges,
        pub primitive_vertices: u64,
        pub screen_vertices: u64,
        pub triangles: u64,
        pub lines: u64,
        /// Immutable VB, transient VB (including GUI), IB, texture patches.
        pub upload_bytes: [u64; 4],
        pub wall_ns: u64,
        pub cpu_sample_wall_ns: u64,
        pub cpu_sample_ns: u64,
        pub cpu_samples: u64,
        pub cpu_unavailable: u64,
    }
    impl Counters {
        pub fn draw(
            &mut self,
            indexed: bool,
            vertices: u32,
            triangles: u32,
            lines: u32,
            screen: bool,
        ) {
            if indexed {
                self.indexed_vertices.add(vertices);
            } else {
                self.primitive_vertices += u64::from(vertices);
            }
            if screen {
                self.screen_vertices += u64::from(vertices);
            }
            self.triangles += u64::from(triangles);
            self.lines += u64::from(lines);
        }
    }

    struct Window {
        counters: [Counters; PASSES.len()],
        started: u64,
        presented: u64,
        aborted: u64,
        since: Instant,
    }
    impl Default for Window {
        fn default() -> Self {
            Self {
                counters: std::array::from_fn(|_| Counters::default()),
                started: 0,
                presented: 0,
                aborted: 0,
                since: Instant::now(),
            }
        }
    }
    struct State {
        window: Window,
        pass: SubmitPass,
        clock: Option<Instant>,
        sampled: bool,
        cpu: Option<std::time::Duration>,
        sequence: u64,
    }
    pub(crate) struct Profile {
        enabled: bool,
        state: RefCell<State>,
    }
    const _: () = assert!(size_of::<Profile>() <= 16 * 1024);
    impl Default for Profile {
        fn default() -> Self {
            Self {
                enabled: crate::log_stats_enabled(),
                state: RefCell::new(State {
                    window: Window::default(),
                    pass: SubmitPass::Outside,
                    clock: None,
                    sampled: false,
                    cpu: None,
                    sequence: 0,
                }),
            }
        }
    }
    impl State {
        fn switch(&mut self, pass: SubmitPass) {
            let now = Instant::now();
            let cpu = if self.sampled { cpu_time() } else { None };
            if let Some(clock) = self.clock {
                let wall = now.duration_since(clock).as_nanos() as u64;
                let counter = &mut self.window.counters[self.pass as usize];
                counter.wall_ns += wall;
                if self.sampled {
                    if let Some(elapsed) = cpu
                        .zip(self.cpu)
                        .and_then(|(new, old)| new.checked_sub(old))
                    {
                        counter.cpu_sample_wall_ns += wall;
                        counter.cpu_sample_ns += elapsed.as_nanos() as u64;
                        counter.cpu_samples += 1;
                    } else {
                        counter.cpu_unavailable += 1;
                    }
                }
            }
            self.pass = pass;
            self.clock = (pass != SubmitPass::Outside).then_some(now);
            self.cpu = cpu;
        }
    }
    #[cfg(windows)]
    fn cpu_time() -> Option<std::time::Duration> {
        crate::thread_cpu_time()
    }
    #[cfg(not(windows))]
    fn cpu_time() -> Option<std::time::Duration> {
        None
    }

    impl Profile {
        pub fn count(&self, update: impl FnOnce(&mut Counters)) {
            if self.enabled {
                let mut state = self.state.borrow_mut();
                let pass = state.pass as usize;
                update(&mut state.window.counters[pass]);
            }
        }
        pub fn begin(&self) {
            if !self.enabled {
                return;
            }
            let mut state = self.state.borrow_mut();
            state.sampled = state.sequence.is_multiple_of(64);
            state.sequence = state.sequence.wrapping_add(1);
            state.window.started += 1;
            state.switch(SubmitPass::Begin);
        }
        pub fn pass(&self, pass: SubmitPass) {
            if self.enabled {
                self.state.borrow_mut().switch(pass);
            }
        }
        pub fn end(&self, presented: bool) {
            if !self.enabled {
                return;
            }
            let mut state = self.state.borrow_mut();
            state.switch(SubmitPass::Outside);
            state.sampled = false;
            if presented {
                state.window.presented += 1;
            } else {
                state.window.aborted += 1;
            }
        }
        #[cfg(windows)]
        pub fn log(&self) {
            if !self.enabled {
                return;
            }
            let mut state = self.state.borrow_mut();
            let window = std::mem::take(&mut state.window);
            log::info!(
                "D3D9 submit_window v=1 seconds={:.6} started={} presented={} aborted={} cpu_sample_period=64 gpu_timing=not_instrumented",
                window.since.elapsed().as_secs_f64(),
                window.started,
                window.presented,
                window.aborted
            );
            for (pass, c) in PASSES.iter().zip(window.counters) {
                log::info!(
                    "D3D9 submit pass={pass:?} com={:?} failures={:?} cache_requests={:?} state_requests={} transform_requests={} dip_num_vertices={} dip_ranges={} dip_min={:?} dip_max={} dip_p50_upper={:?} dip_p95_upper={:?} dp_vertices={} screen_vertices={} triangles={} lines={} upload_bytes={:?} wall_ms={:.6} sample_wall_ms={:.6} sample_cpu_ms={:.6} cpu_samples={} cpu_unavailable={}",
                    c.calls,
                    c.failures,
                    c.cache_requests,
                    c.state_requests,
                    c.transform_requests,
                    c.indexed_vertices.sum,
                    c.indexed_vertices.count,
                    c.indexed_vertices.min,
                    c.indexed_vertices.max,
                    c.indexed_vertices.percentile_upper(50),
                    c.indexed_vertices.percentile_upper(95),
                    c.primitive_vertices,
                    c.screen_vertices,
                    c.triangles,
                    c.lines,
                    c.upload_bytes,
                    c.wall_ns as f64 / 1e6,
                    c.cpu_sample_wall_ns as f64 / 1e6,
                    c.cpu_sample_ns as f64 / 1e6,
                    c.cpu_samples,
                    c.cpu_unavailable
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn vertex_ranges_count_driver_span_including_gaps() {
            let mut counters = Counters::default();
            // Two dense quads; a sparse two-quad IB spanning 100 vertices; one
            // pretransformed GUI triangle. No unique/transformed count is inferred.
            counters.draw(true, 4, 2, 0, false);
            counters.draw(true, 4, 2, 0, false);
            counters.draw(true, 100, 4, 0, false);
            counters.draw(false, 3, 1, 0, true);
            assert_eq!(counters.indexed_vertices.sum, 108);
            assert_eq!(counters.indexed_vertices.count, 3);
            assert_eq!(counters.indexed_vertices.min, Some(4));
            assert_eq!(counters.indexed_vertices.max, 100);
            assert_eq!(counters.indexed_vertices.percentile_upper(50), Some(4));
            assert_eq!(counters.indexed_vertices.percentile_upper(95), Some(128));
            assert_eq!(
                (
                    counters.primitive_vertices,
                    counters.screen_vertices,
                    counters.triangles
                ),
                (3, 3, 9)
            );
            assert_eq!(Ranges::default().percentile_upper(95), None);
            let mut largest = Ranges::default();
            largest.add(u32::MAX);
            assert_eq!(largest.percentile_upper(95), Some(1u64 << 32));
        }

        #[test]
        fn aborted_frame_does_not_attribute_later_uploads_to_its_last_pass() {
            let profile = Profile {
                enabled: true,
                ..Profile::default()
            };
            profile.count(|c| c.upload_bytes[0] += 128);
            profile.begin();
            profile.pass(SubmitPass::Solid);
            profile.count(|c| c.draw(true, 8, 4, 0, false));
            profile.end(false);
            profile.count(|c| c.upload_bytes[0] += 256);
            profile.begin();
            profile.pass(SubmitPass::StaticEntities);
            profile.count(|c| c.draw(true, 4, 2, 0, false));
            profile.end(true);
            let state = profile.state.borrow();
            let window = &state.window;
            assert_eq!(
                (window.started, window.presented, window.aborted),
                (2, 1, 1)
            );
            assert_eq!(
                window.counters[SubmitPass::Outside as usize].upload_bytes[0],
                384
            );
            assert_eq!(
                window.counters[SubmitPass::Solid as usize]
                    .indexed_vertices
                    .sum,
                8
            );
            assert_eq!(
                window.counters[SubmitPass::StaticEntities as usize]
                    .indexed_vertices
                    .sum,
                4
            );
            assert_eq!(state.pass, SubmitPass::Outside);
            assert!(state.clock.is_none());
        }
    }
}
