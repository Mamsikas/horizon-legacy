//! Bulk sections share the compaction thread. A charged permit bounds queued
//! input/results; edits to present/pending sections use the synchronous path.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
const LIMIT: usize = 8 << 20;
pub(super) const MAX_JOBS: usize = 64;
pub(super) struct Job {
    key: SectionKey,
    ticket: u64,
    mesh: SectionMesh,
    permit: Permit,
    epoch: usize,
    submitted: Instant,
    recording: bool,
}
struct Permit {
    quota: Arc<AtomicUsize>,
    jobs: Arc<AtomicUsize>,
    bytes: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.quota.fetch_sub(self.bytes, Ordering::Relaxed);
        self.jobs.fetch_sub(1, Ordering::Relaxed);
    }
}
pub(in super::super) struct Ready {
    pub key: SectionKey,
    pub ticket: u64,
    pub result: std::result::Result<convert::Converted, String>,
    pub queue: Duration,
    pub conversion: Duration,
    _permit: Permit,
}
pub(super) fn prepare(
    job: Job,
    layout: &dx::AtlasSet,
    scratch: &mut convert::Scratch,
    epoch: &AtomicUsize,
    hardware: bool,
) -> Ready {
    let queue = job.submitted.elapsed();
    crate::spans::record_here(job.recording);
    let started = Instant::now();
    let result = {
        let _span = crate::spans::guard("d3d9_section_convert");
        if job.epoch == epoch.load(Ordering::Relaxed) {
            scratch.prepare(layout, job.key, &job.mesh, hardware)
        } else {
            Err("discarded section from the preceding world".into())
        }
    };
    let conversion = started.elapsed();
    let _ = crate::spans::take_raw();
    crate::spans::record_here(false);
    Ready {
        key: job.key,
        ticket: job.ticket,
        result,
        queue,
        conversion,
        _permit: job.permit,
    }
}
/// Give fast render loops a bounded group of arrivals, rather than one native
/// VB per completed section followed by decode/reupload compaction. Edits never
/// enter this worker. The existing permit covers results while the group waits.
pub(super) fn prepare_batch(
    first: Job,
    input: &std::sync::mpsc::Receiver<compact::Work>,
    layout: &dx::AtlasSet,
    scratch: &mut convert::Scratch,
    epoch: &AtomicUsize,
    hardware: bool,
) -> (Vec<Ready>, Option<compact::Work>) {
    let mut ready = Vec::new();
    let mut bytes = 0;
    let mut deadline = None;
    let mut next = first;
    loop {
        let value = prepare(next, layout, scratch, epoch, hardware);
        bytes += value.result.as_ref().map_or(0, |v| v.gpu.bytes().len());
        ready.push(value);
        let until = *deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(2));
        let remaining = until.saturating_duration_since(Instant::now());
        if ready.len() >= 32 || bytes >= 2 << 20 || remaining.is_zero() {
            return (ready, None);
        }
        match input.recv_timeout(remaining) {
            Ok(compact::Work::Upload(job)) => next = job,
            Ok(work) => return (ready, Some(work)),
            Err(_) => return (ready, None),
        }
    }
}
impl Compactor {
    fn charge(mesh: &SectionMesh) -> usize {
        let count = mesh.solid.len() + mesh.translucent.len();
        count
            .saturating_mul(size_of::<TerrainVertex>() * 8)
            .saturating_add(mesh.block_entities.len() * 64)
            .saturating_add(1024)
    }
    pub(in super::super) fn upload_room(&self, mesh: &SectionMesh) -> bool {
        // Oversized jobs must still take the synchronous path: waiting for a
        // job that can never fit would deadlock loading. This check needs only
        // lengths; do not rescan every UV each frame while the quota is full.
        let bytes = Self::charge(mesh);
        (mesh.solid.is_empty() && mesh.translucent.is_empty())
            || bytes > LIMIT
            || self.upload_count.load(Ordering::Relaxed) < MAX_JOBS
                && self
                    .upload_quota
                    .load(Ordering::Relaxed)
                    .saturating_add(bytes)
                    <= LIMIT
    }
    pub(in super::super) fn try_upload(
        &self,
        key: SectionKey,
        ticket: u64,
        mesh: &SectionMesh,
    ) -> bool {
        // The charge covers source, grouped/full vertices, shadow, encoded GPU
        // data and packets. Repeating UVs may expand; those stay synchronous.
        if mesh.solid.is_empty() && mesh.translucent.is_empty()
            || self.upload_count.load(Ordering::Relaxed) >= MAX_JOBS
            || mesh
                .solid
                .iter()
                .chain(&mesh.translucent)
                .any(|v| v.uv.iter().any(|&u| u > crate::mesher::UV_STEPS as u16))
        {
            return false;
        }
        let bytes = Self::charge(mesh);
        let mut used = self.upload_quota.load(Ordering::Relaxed);
        loop {
            let Some(next) = used.checked_add(bytes).filter(|&n| n <= LIMIT) else {
                return false;
            };
            match self.upload_quota.compare_exchange_weak(
                used,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => used = current,
            }
        }
        let permit = Permit {
            quota: self.upload_quota.clone(),
            jobs: self.upload_count.clone(),
            bytes,
        };
        // Submission is render-thread-only; background permit drops only lower
        // this count. Bound job slots too, so small meshes cannot overflow the
        // channel and spill into synchronous conversion despite spare byte quota.
        self.upload_count.fetch_add(1, Ordering::Relaxed);
        let input = SectionMesh {
            solid: mesh.solid.clone(),
            translucent: mesh.translucent.clone(),
            sides: mesh.sides,
            bounds: mesh.bounds,
            visibility: mesh.visibility,
            block_entities: mesh.block_entities.clone(),
            ..Default::default()
        };
        self.jobs
            .try_send(compact::Work::Upload(Job {
                key,
                ticket,
                mesh: input,
                permit,
                epoch: self.upload_epoch.load(Ordering::Relaxed),
                submitted: Instant::now(),
                recording: crate::spans::recording_here(),
            }))
            .is_ok()
    }
    pub(in super::super) fn take_upload(&mut self) -> Option<Ready> {
        if self.upload_ready.is_empty() {
            let batch = self.upload_done.try_recv().ok()?;
            self.upload_batches[0] += 1;
            self.upload_batches[1] = self.upload_batches[1].max(batch.len() as u64);
            self.upload_ready.extend(batch);
        }
        self.upload_ready.pop_front()
    }
    pub(in super::super) fn batch_stats(&self) -> [u64; 2] {
        self.upload_batches
    }
    pub(in super::super) fn reserved_uploads(&self) -> usize {
        self.upload_quota.load(Ordering::Relaxed)
    }
    pub(in super::super) fn cancel_uploads(&self) {
        self.upload_epoch.fetch_add(1, Ordering::Relaxed);
    }
}
