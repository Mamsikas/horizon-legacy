//! Sample the HAL command stream without flushing it or waiting for a query.
use crate::{SubmitPass, benchmark};
use std::{mem::size_of, time::Instant};
use windows::{
    Win32::{
        Foundation::{S_FALSE, S_OK},
        Graphics::Direct3D9::*,
    },
    core::Interface,
};

const SAMPLE_PERIOD: u64 = 64;
const RING_SIZE: usize = 3;
const POLL_DELAY: u64 = 2;
const MAX_PENDING: u64 = SAMPLE_PERIOD * RING_SIZE as u64;

#[derive(Default)]
pub(super) struct Probe {
    initialized: bool,
    frame: u64,
    slots: Vec<Slot>,
    active: Option<usize>,
}

struct Slot {
    queries: Queries,
    pending: Option<Pending>,
    solid_marks: u8,
}

struct Pending {
    frame: u64,
    bench: Option<benchmark::Slot>,
    submitted: Instant,
    last_pending_us: Option<u128>,
}

enum Queries {
    Timestamps {
        first: IDirect3DQuery9,
        last: IDirect3DQuery9,
        solid_first: IDirect3DQuery9,
        solid_last: IDirect3DQuery9,
        frequency: IDirect3DQuery9,
        disjoint: IDirect3DQuery9,
    },
    Event(IDirect3DQuery9),
}

enum Sample {
    Timestamp {
        frame_ms: f64,
        solid_ms: Option<f64>,
    },
    Disjoint,
    Completion,
}

impl Queries {
    fn timestamps(device: &IDirect3DDevice9) -> windows::core::Result<Self> {
        // SAFETY: optional query creation on the device's owner thread.
        let create = |kind| unsafe { device.CreateQuery(kind) };
        Ok(Self::Timestamps {
            first: create(D3DQUERYTYPE_TIMESTAMP)?,
            last: create(D3DQUERYTYPE_TIMESTAMP)?,
            solid_first: create(D3DQUERYTYPE_TIMESTAMP)?,
            solid_last: create(D3DQUERYTYPE_TIMESTAMP)?,
            frequency: create(D3DQUERYTYPE_TIMESTAMPFREQ)?,
            disjoint: create(D3DQUERYTYPE_TIMESTAMPDISJOINT)?,
        })
    }

    fn begin(&self) -> windows::core::Result<()> {
        if let Self::Timestamps {
            first, disjoint, ..
        } = self
        {
            issue(disjoint, D3DISSUE_BEGIN)?;
            issue(first, D3DISSUE_END)?;
        }
        Ok(())
    }

    fn end(&self) -> windows::core::Result<()> {
        match self {
            Self::Timestamps {
                last,
                frequency,
                disjoint,
                ..
            } => {
                issue(last, D3DISSUE_END)?;
                issue(frequency, D3DISSUE_END)?;
                issue(disjoint, D3DISSUE_END)
            }
            Self::Event(query) => issue(query, D3DISSUE_END),
        }
    }

    fn read(&self, solid: bool) -> windows::core::Result<Option<Sample>> {
        match self {
            Self::Timestamps {
                first,
                last,
                solid_first,
                solid_last,
                frequency,
                disjoint,
            } => {
                let Some(disjoint) = read::<i32>(disjoint)? else {
                    return Ok(None);
                };
                if disjoint != 0 {
                    return Ok(Some(Sample::Disjoint));
                }
                let (Some(first), Some(last), Some(freq)) = (
                    read::<u64>(first)?,
                    read::<u64>(last)?,
                    read::<u64>(frequency)?,
                ) else {
                    return Ok(None);
                };
                if freq == 0 || last < first {
                    return Ok(Some(Sample::Disjoint));
                }
                let ms = |ticks| ticks as f64 * 1000.0 / freq as f64;
                let solid_ms = if solid {
                    let (Some(start), Some(end)) =
                        (read::<u64>(solid_first)?, read::<u64>(solid_last)?)
                    else {
                        return Ok(None);
                    };
                    end.checked_sub(start).map(ms)
                } else {
                    None
                };
                Ok(Some(Sample::Timestamp {
                    frame_ms: ms(last - first),
                    solid_ms,
                }))
            }
            Self::Event(query) => Ok(read::<i32>(query)?
                .filter(|&done| done != 0)
                .map(|_| Sample::Completion)),
        }
    }
}

fn issue(query: &IDirect3DQuery9, flag: u32) -> windows::core::Result<()> {
    // SAFETY: each query stays on the creating thread. Flags match its type.
    unsafe { query.Issue(flag) }
}

fn read<T: Default>(query: &IDirect3DQuery9) -> windows::core::Result<Option<T>> {
    let mut value = T::default();
    // SAFETY: callers use DWORD-sized BOOL or UINT64 matching the query type.
    // The generated wrapper treats S_FALSE as success, so keep the raw HRESULT.
    let status = unsafe {
        (Interface::vtable(query).GetData)(
            Interface::as_raw(query),
            (&mut value as *mut T).cast(),
            size_of::<T>() as u32,
            0,
        )
    };
    match status {
        S_OK => Ok(Some(value)),
        S_FALSE => Ok(None),
        status => Err(windows::core::Error::from_hresult(status)),
    }
}

impl Probe {
    pub(super) fn begin(&mut self, device: &IDirect3DDevice9) {
        if !self.initialized {
            self.initialized = true;
            let mut timestamps = true;
            for _ in 0..RING_SIZE {
                let queries = if timestamps {
                    Queries::timestamps(device).ok()
                } else {
                    None
                };
                let queries = match queries {
                    Some(queries) => queries,
                    None if self.slots.is_empty() || !timestamps => {
                        timestamps = false;
                        // SAFETY: optional EVENT queries on this same HAL.
                        let Ok(query) = (unsafe { device.CreateQuery(D3DQUERYTYPE_EVENT) }) else {
                            break;
                        };
                        Queries::Event(query)
                    }
                    None => break,
                };
                self.slots.push(Slot {
                    queries,
                    pending: None,
                    solid_marks: 0,
                });
            }
            let mode = if self.slots.is_empty() {
                "unsupported"
            } else if timestamps {
                "timestamp"
            } else {
                "event"
            };
            log::info!(
                "D3D9 gpu_queries mode={mode} ring={} sample_period={SAMPLE_PERIOD}",
                self.slots.len()
            );
        }
        if self.slots.is_empty() {
            return;
        }
        self.frame += 1;
        let frame = self.frame;
        for slot in &mut self.slots {
            let Some(sample) = &mut slot.pending else {
                continue;
            };
            let delay = frame - sample.frame;
            if delay < POLL_DELAY {
                continue;
            }
            let poll_start = sample.submitted.elapsed().as_micros();
            let bench_slot = sample.bench.map(|slot| slot.number);
            let arm = sample.bench.map(|slot| slot.arm());
            let measuring = sample.bench.is_some_and(|slot| slot.measuring);
            match slot.queries.read(slot.solid_marks == 2) {
                Ok(Some(Sample::Timestamp { frame_ms, solid_ms })) => {
                    log::info!(
                        "D3D9 gpu_sample frame={} frame_ms={frame_ms:.6} solid_ms={solid_ms:?} bench_slot={bench_slot:?} arm={arm:?} measuring={measuring}",
                        sample.frame
                    );
                }
                Ok(Some(Sample::Completion)) => {
                    let age = sample.submitted.elapsed().as_micros();
                    log::info!(
                        "D3D9 gpu_event frame={} completion_upper_us={age} last_pending_us={:?} poll_frames={delay} bench_slot={bench_slot:?} arm={arm:?} measuring={measuring}",
                        sample.frame,
                        sample.last_pending_us
                    );
                }
                Ok(Some(Sample::Disjoint)) => {
                    log::debug!("D3D9 gpu_sample frame={} discarded=disjoint", sample.frame);
                }
                Ok(None) if delay < MAX_PENDING => {
                    sample.last_pending_us = Some(poll_start);
                    continue;
                }
                Ok(None) => log::warn!("D3D9 gpu_sample frame={} discarded=timeout", sample.frame),
                Err(error) => {
                    log::warn!("D3D9 gpu_sample frame={} discarded={error}", sample.frame)
                }
            }
            slot.pending = None;
        }
        if frame.is_multiple_of(SAMPLE_PERIOD)
            && let Some(index) = self.slots.iter().position(|slot| slot.pending.is_none())
        {
            let slot = &mut self.slots[index];
            match slot.queries.begin() {
                Ok(()) => {
                    slot.solid_marks = 0;
                    self.active = Some(index);
                }
                Err(error) => log::warn!("D3D9 gpu query start failed ({error})"),
            }
        }
    }

    pub(super) fn pass(&mut self, pass: SubmitPass) {
        let Some(index) = self.active else { return };
        let slot = &mut self.slots[index];
        if let Queries::Timestamps {
            solid_first,
            solid_last,
            ..
        } = &slot.queries
        {
            let query = match (slot.solid_marks, pass) {
                (0, SubmitPass::Solid) => solid_first,
                (1, SubmitPass::DynamicEntities) => solid_last,
                _ => return,
            };
            if issue(query, D3DISSUE_END).is_ok() {
                slot.solid_marks += 1;
            }
        }
    }

    pub(super) fn end(&mut self, bench: Option<benchmark::Slot>) {
        let Some(index) = self.active.take() else {
            return;
        };
        let slot = &mut self.slots[index];
        let submitted = Instant::now();
        match slot.queries.end() {
            Ok(()) => {
                slot.pending = Some(Pending {
                    frame: self.frame,
                    bench,
                    submitted,
                    last_pending_us: None,
                })
            }
            Err(error) => log::warn!("D3D9 gpu query end failed ({error})"),
        }
    }

    pub(super) fn abort(&mut self) {
        // A partial disjoint interval must be ended before this bundle can be
        // reused. Dropping the bundles also releases them ahead of device reset.
        if self.active.is_some()
            || self.slots.iter().any(|slot| {
                slot.pending
                    .as_ref()
                    .is_some_and(|sample| sample.frame == self.frame)
            })
        {
            *self = Self::default();
        }
    }
}
