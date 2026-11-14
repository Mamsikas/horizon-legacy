//! Exact reuse of small alpha HUD and depth/scissor inventory ranges. Depth
//! clears stay outside the cache. Pictures/glint/invert retain the normal path;
//! newly present atlas tiles invalidate mappings, pixel-only patches do not.
use super::*;
const CHUNK: usize = 192; // complete triangles
const SLOTS: usize = 128;
const BUDGET: usize = 2 << 20;

#[derive(Clone, Copy)]
pub(super) struct GuiRun<'a> {
    pub source: &'a [crate::hud::HudVertex],
    pub pixels: &'a Pixels,
    pub state: DrawState,
    pub picture: Option<u64>,
    pub cache: bool,
}
#[derive(Default)]
struct Entry {
    source: Vec<crate::hud::HudVertex>,
    state: Option<DrawState>,
    revision: u64,
    vertices: Vec<dx::Vertex>,
    commands: Vec<GuiCommand>,
}
impl Entry {
    fn bytes(&self) -> usize {
        self.source.capacity() * size_of::<crate::hud::HudVertex>()
            + self.vertices.capacity() * size_of::<dx::Vertex>()
            + self.commands.capacity() * size_of::<GuiCommand>()
    }
}
#[derive(Default)]
pub(in super::super) struct GuiCache {
    entries: Vec<Entry>,
    at: usize,
    retained: usize,
    pub hits: usize,
    pub misses: usize,
}
impl GuiCache {
    pub fn begin(&mut self) {
        self.at = 0;
        self.hits = 0;
        self.misses = 0;
    }
    pub fn finish(&mut self) {
        self.retained -= self.entries[self.at..]
            .iter()
            .map(Entry::bytes)
            .sum::<usize>();
        self.entries.truncate(self.at);
    }
    pub fn bytes(&self) -> usize {
        self.retained + self.entries.capacity() * size_of::<Entry>()
    }
    pub(super) fn append(
        &mut self,
        run: GuiRun<'_>,
        batches: &mut GuiBatches,
        vertices: &mut Vec<dx::Vertex>,
        commands: &mut Vec<GuiCommand>,
    ) {
        if !run.cache {
            build(&run, batches, vertices, commands);
            return;
        }
        for source in run.source.chunks(CHUNK) {
            let slice = GuiRun { source, ..run };
            if self.at >= SLOTS {
                build(&slice, batches, vertices, commands);
                continue;
            }
            if self.at == self.entries.len() {
                self.entries.push(Entry::default());
            }
            let entry = &mut self.entries[self.at];
            self.at += 1;
            let same = entry.state == Some(run.state)
                && entry.revision == run.pixels.revision()
                && bytemuck::cast_slice::<_, u8>(&entry.source)
                    == bytemuck::cast_slice::<_, u8>(source);
            if same {
                self.hits += 1;
            } else {
                self.misses += 1;
                self.retained -= entry.bytes();
                entry.vertices.clear();
                entry.commands.clear();
                build(&slice, batches, &mut entry.vertices, &mut entry.commands);
                entry.source.clear();
                entry.source.extend_from_slice(source);
                entry.state = Some(run.state);
                entry.revision = run.pixels.revision();
                self.retained += entry.bytes();
            }
            let offset = vertices.len() as u32;
            vertices.extend_from_slice(&entry.vertices);
            for command in &entry.commands {
                match command {
                    GuiCommand::Draw {
                        page,
                        state,
                        picture,
                        range,
                    } => {
                        let range = range.start + offset..range.end + offset;
                        // Chunk boundaries are CPU cache boundaries, not draw barriers.
                        if let Some(GuiCommand::Draw {
                            page: p,
                            state: s,
                            picture: i,
                            range: r,
                        }) = commands.last_mut()
                            && p == page
                            && s == state
                            && i == picture
                            && r.end == range.start
                        {
                            r.end = range.end;
                        } else {
                            commands.push(GuiCommand::Draw {
                                page: *page,
                                state: *state,
                                picture: *picture,
                                range,
                            });
                        }
                    }
                    GuiCommand::ClearDepth => commands.push(GuiCommand::ClearDepth),
                }
            }
            // Retain only bounded prepared data; the current frame still draws everything.
            if self.retained > BUDGET {
                self.retained -= entry.bytes();
                *entry = Entry::default();
            }
        }
    }
}
fn build(
    run: &GuiRun<'_>,
    batches: &mut GuiBatches,
    vertices: &mut Vec<dx::Vertex>,
    commands: &mut Vec<GuiCommand>,
) {
    batches.begin(run.state);
    for tri in run.source.chunks_exact(3) {
        if tri[0].uv[0] >= 2.0 {
            continue;
        } // optional foil over an already-emitted base item
        let input: [dx::Vertex; 3] = std::array::from_fn(|i| {
            let v = &tri[i];
            dx::Vertex {
                pos: [v.pos[0], v.pos[1], v.depth],
                uv: v.uv,
                color: dx::rgba_to_argb(v.color),
                light: [0.0; 2],
            }
        });
        run.pixels.polygon(&input, |page, p| batches.push(page, p));
    }
    batches.finish(vertices, commands, run.picture);
}
