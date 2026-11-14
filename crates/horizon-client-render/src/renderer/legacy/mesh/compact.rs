//! Consolidate small immutable VBs without decoding them on the render thread.
//! One bounded CPU job/result, no GPU objects or gameplay state cross the thread.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
type Snapshot = (SectionKey, u64, Arc<dx::MeshData>);
pub(super) struct Job {
    region: RegionKey,
    sections: Vec<Snapshot>,
    resident: bool,
}
struct Prepared {
    region: RegionKey,
    sections: Vec<(SectionKey, u64, Range<u32>)>,
    resident: bool,
    encoded: dx::EncodedVertices,
}
pub(super) enum Work {
    Compact(Job),
    Upload(upload::Job),
}
pub(in super::super) struct Compactor {
    pub(super) jobs: SyncSender<Work>,
    done: Receiver<std::result::Result<Prepared, String>>,
    pub(super) upload_done: Receiver<upload::Ready>,
    pub(super) upload_quota: Arc<AtomicUsize>,
    pub(super) upload_epoch: Arc<AtomicUsize>,
    busy: bool,
}
impl Compactor {
    pub(in super::super) fn new(layout: Arc<dx::AtlasSet>, hardware: bool) -> Option<Self> {
        let (jobs, input) = mpsc::sync_channel::<Work>(64);
        let (output, done) = mpsc::sync_channel(1);
        let (uploads, upload_done) = mpsc::sync_channel(64);
        let upload_quota = Arc::new(AtomicUsize::new(0));
        let upload_epoch = Arc::new(AtomicUsize::new(0));
        let epoch = upload_epoch.clone();
        std::thread::Builder::new()
            .name("d3d9 section staging".into())
            .spawn(move || {
                let mut scratch = convert::Scratch::default();
                let mut combined = Vec::<dx::Vertex>::new();
                let mut decoded = Vec::new();
                while let Ok(work) = input.recv() {
                    let job = match work {
                        Work::Upload(job) => {
                            let ready =
                                upload::prepare(job, &layout, &mut scratch, &epoch, hardware);
                            if uploads.send(ready).is_err() {
                                break;
                            }
                            continue;
                        }
                        Work::Compact(job) => job,
                    };
                    let prepare = || -> std::result::Result<Prepared, String> {
                        let _span = crate::spans::guard("d3d9_mesh_compact");
                        let mut sections = Vec::new();
                        combined.clear();
                        for (key, revision, data) in job.sections {
                            data.decode(&mut decoded)?;
                            let first = combined.len() as u32;
                            combined.extend_from_slice(&decoded);
                            sections.push((key, revision, first..combined.len() as u32));
                        }
                        Ok(Prepared {
                            region: job.region,
                            sections,
                            resident: job.resident,
                            encoded: dx::EncodedVertices::new(&combined, hardware),
                        })
                    };
                    let prepared = prepare();
                    let _ = crate::spans::take_raw();
                    if output.send(prepared).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Self {
            jobs,
            done,
            upload_done,
            upload_quota,
            upload_epoch,
            busy: false,
        })
    }
}
impl Renderer {
    pub(in super::super) fn collect_compaction(&mut self) -> Result<()> {
        let Some(worker) = &mut self.compactor else {
            return Ok(());
        };
        let Ok(result) = worker.done.try_recv() else {
            return Ok(());
        };
        worker.busy = false;
        let prepared = match result {
            Ok(value) => value,
            Err(error) => {
                log::warn!("D3D9 compaction decode: {error}");
                return Ok(());
            }
        };
        if prepared.sections.iter().any(|(key, revision, _)| {
            self.sections
                .get(key)
                .is_none_or(|s| s.revision != *revision || s.gpu.is_empty() == prepared.resident)
        }) {
            return Ok(());
        }
        if !prepared.resident
            && (self.gpu_mesh_bytes + prepared.encoded.bytes().len() as u64 > self.mesh_budget
                || prepared
                    .sections
                    .iter()
                    .any(|(key, _, _)| !self.occlusion.contains(*key)))
        {
            return Ok(());
        }
        // Allocate first: a failed optimization must keep the old scene drawable.
        let uploaded = self.device.mesh_encoded(&prepared.encoded);
        let mesh = match uploaded {
            Ok(mesh) => mesh,
            Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
            Err(error) => {
                self.compact_scan = self.frames + 120;
                self.prefetch_scan = self.frames + 120;
                log::warn!("D3D9 background upload: {error}; keeping old buffers/shadows");
                return Ok(());
            }
        };
        self.next_arena += 1;
        let id = self.next_arena;
        let mut keys = Vec::new();
        for (key, _, source) in prepared.sections {
            self.release_gpu_section(key);
            self.sections.get_mut(&key).unwrap().gpu.push(GpuSlice {
                arena: id,
                first: source.start,
                source: 0..source.end - source.start,
            });
            keys.push(key);
        }
        self.gpu_mesh_bytes += mesh.bytes();
        self.arenas.insert(
            id,
            Arena {
                mesh,
                region: prepared.region,
                live: prepared.encoded.len() as u32,
                last_used: self.frames + 1,
                sections: keys,
            },
        );
        if prepared.resident {
            self.compactions += 1;
        } else {
            self.eager_sections += self.arenas[&id].sections.len() as u64;
        }
        Ok(())
    }
    pub(in super::super) fn schedule_compaction(&mut self) {
        if self
            .compactor
            .as_ref()
            .is_none_or(|w| w.busy || w.upload_quota.load(Ordering::Relaxed) != 0)
        {
            return;
        }
        if self.schedule_prefetch() {
            return;
        }
        if self.frames < self.compact_scan + 8 {
            return;
        }
        self.compact_scan = self.frames;
        let limit = self.device.max_vertices(); // at most 2 MiB of prepared vertices
        let mut regions = HashMap::<(RegionKey, dx::VertexEncoding), (usize, usize)>::default();
        for arena in self.arenas.values() {
            let entry = regions
                .entry((arena.region, arena.mesh.encoding()))
                .or_default();
            entry.0 += 1;
            entry.1 += arena.live as usize;
        }
        let Some(((region, encoding), _)) = regions
            .into_iter()
            .filter(|(_, (pages, live))| *pages > live.div_ceil(limit) * 2 + 1)
            .max_by_key(|(_, (pages, live))| pages - live.div_ceil(limit))
        else {
            return;
        };
        let mut pages: Vec<_> = self
            .arenas
            .iter()
            .filter(|(_, a)| a.region == region && a.mesh.encoding() == encoding)
            .collect();
        pages.sort_unstable_by_key(|(_, a)| a.live);
        let mut chosen = FxHashSet::default();
        let mut vertices = 0;
        let mut covered = 0;
        for (&id, arena) in pages {
            let extra: Vec<_> = arena
                .sections
                .iter()
                .copied()
                .filter(|key| {
                    !chosen.contains(key)
                        && self
                            .sections
                            .get(key)
                            .is_some_and(|s| s.gpu.iter().any(|g| g.arena == id))
                })
                .collect();
            let count: usize = extra.iter().map(|k| self.sections[k].vertices.len()).sum();
            if vertices + count <= limit {
                vertices += count;
                chosen.extend(extra);
                covered += 1;
            }
        }
        if covered < 3 || vertices == 0 {
            return;
        }
        let sections = chosen
            .into_iter()
            .map(|key| {
                let section = &self.sections[&key];
                (key, section.revision, section.vertices.clone())
            })
            .collect();
        let worker = self.compactor.as_mut().unwrap();
        worker.busy = worker
            .jobs
            .try_send(Work::Compact(Job {
                region,
                sections,
                resident: true,
            }))
            .is_ok();
    }
    fn schedule_prefetch(&mut self) -> bool {
        if self.frames < self.prefetch_scan || self.occlusion.reach().is_none() {
            return false;
        }
        self.prefetch_scan = self.frames + 32;
        let limit = self.device.max_vertices().min(
            (self.mesh_budget.saturating_sub(self.gpu_mesh_bytes) / size_of::<dx::Vertex>() as u64)
                as usize,
        );
        let ready = |key: SectionKey, s: &Section| {
            s.gpu.is_empty()
                && !s.vertices.is_empty()
                && s.vertices.len() <= limit
                && self.frames >= s.retry_after
                && self.occlusion.contains(key)
        };
        let Some((key, _)) = self.sections.iter().find(|(k, s)| ready(*k, s)) else {
            return false;
        };
        let region = region_key(key);
        let encoding = self.sections[&key].vertices.encoding();
        let mut count = 0;
        let sections = self
            .sections
            .iter()
            .filter_map(|(key, section)| {
                if region_key(key) == region
                    && (!self.device.compact_vertices() || section.vertices.encoding() == encoding)
                    && ready(key, section)
                    && count + section.vertices.len() <= limit
                {
                    count += section.vertices.len();
                    Some((key, section.revision, section.vertices.clone()))
                } else {
                    None
                }
            })
            .collect();
        let worker = self.compactor.as_mut().unwrap();
        worker.busy = worker
            .jobs
            .try_send(Work::Compact(Job {
                region,
                sections,
                resident: false,
            }))
            .is_ok();
        self.prefetch_scan = self.frames + 1;
        worker.busy
    }
}
