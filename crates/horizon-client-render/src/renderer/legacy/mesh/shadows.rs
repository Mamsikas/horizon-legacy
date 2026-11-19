//! Compress immediate updates on the existing worker without delaying their GPU geometry.
use super::*;

impl Compactor {
    pub(in super::super) fn shadow_status(&self) -> (usize, usize, bool, [u64; 3]) {
        (
            self.shadow_queue.len(),
            self.shadow_queue.bytes(),
            self.shadow_busy,
            self.shadow_stats,
        )
    }
}

pub(super) struct Job {
    pub key: SectionKey,
    pub source: Arc<dx::MeshData>,
}
pub(super) struct Ready {
    pub job: Job,
    pub compressed: Option<Arc<dx::MeshData>>,
    pub elapsed: Duration,
}
pub(super) fn prepare(job: Job, scratch: &mut convert::Scratch) -> Ready {
    let start = Instant::now();
    let compressed = scratch.compress(&job.source).map(Arc::new);
    Ready {
        job,
        compressed,
        elapsed: start.elapsed(),
    }
}

impl Renderer {
    pub(in super::super) fn progress_shadows(&mut self) {
        let Some(worker) = &mut self.compactor else {
            return;
        };
        if let Ok(ready) = worker.shadow_done.try_recv() {
            worker.shadow_busy = false;
            let ns = ready.elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
            worker.shadow_stats[0] += 1;
            worker.shadow_stats[1] += ns;
            worker.shadow_stats[2] = worker.shadow_stats[2].max(ns);
            if let Some(section) = self.sections.get_mut(&ready.job.key)
                && Arc::ptr_eq(&section.vertices, &ready.job.source)
                && let Some(compressed) = ready.compressed
            {
                self.mesh_bytes -= section.bytes;
                section.bytes = compressed.bytes();
                section.vertices = compressed;
                self.mesh_bytes += section.bytes;
                // Geometry/revision/native buffers are unchanged by lossless storage compression.
            }
            // Release the last obsolete raw allocation off the frame thread too.
            let _ = worker
                .jobs
                .try_send(compact::Work::ReleaseShadow(ready.job.source));
        }
        if worker.shadow_busy {
            return;
        }
        while let Some(key) = worker.shadow_queue.pop() {
            let Some(section) = self.sections.get(&key) else {
                continue;
            };
            if !section.vertices.compression_pending() {
                continue;
            }
            let bytes = section.bytes as usize;
            let job = Job {
                key,
                source: section.vertices.clone(),
            };
            match worker.jobs.try_send(compact::Work::Shadow(job)) {
                Ok(()) => worker.shadow_busy = true,
                Err(_) => {
                    worker.shadow_queue.admit(key, bytes);
                }
            }
            break;
        }
    }
}
