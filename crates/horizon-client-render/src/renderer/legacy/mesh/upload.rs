//! Bulk sections share the compaction thread. A charged permit bounds queued
//! input/results; edits to present/pending sections use the synchronous path.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
const LIMIT: usize = 8 << 20;
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
    bytes: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.quota.fetch_sub(self.bytes, Ordering::Relaxed);
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
impl Compactor {
    pub(in super::super) fn try_upload(
        &self,
        key: SectionKey,
        ticket: u64,
        mesh: &SectionMesh,
    ) -> bool {
        let count = mesh.solid.len() + mesh.translucent.len();
        // The charge covers source, grouped/full vertices, shadow, encoded GPU
        // data and packets. Repeating UVs may expand; those stay synchronous.
        if count == 0
            || mesh
                .solid
                .iter()
                .chain(&mesh.translucent)
                .any(|v| v.uv.iter().any(|&u| u > crate::mesher::UV_STEPS as u16))
        {
            return false;
        }
        let bytes = count
            .saturating_mul(size_of::<TerrainVertex>() * 8)
            .saturating_add(mesh.block_entities.len() * 64)
            .saturating_add(1024);
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
            bytes,
        };
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
    pub(in super::super) fn take_upload(&self) -> Option<Ready> {
        self.upload_done.try_recv().ok()
    }
    pub(in super::super) fn reserved_uploads(&self) -> usize {
        self.upload_quota.load(Ordering::Relaxed)
    }
    pub(in super::super) fn cancel_uploads(&self) {
        self.upload_epoch.fetch_add(1, Ordering::Relaxed);
    }
}
