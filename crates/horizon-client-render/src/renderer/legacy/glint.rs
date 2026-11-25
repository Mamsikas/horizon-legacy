//! A masked additive pass only for enchanted geometry, using FFP on SWVP too.
use super::*;
pub(super) struct Glint {
    item: [dx::Texture; 2],
    armor: [dx::Texture; 2],
}
impl Glint {
    pub fn new(
        device: &mut dx::Device,
        item: (&[u8], u32, u32),
        armor: (&[u8], u32, u32),
    ) -> Result<Self> {
        let mut make = |(pixels, w, h): (&[u8], u32, u32)| -> Result<_> {
            let bake = device.caps.raw.MaxTextureBlendStages < 3;
            Ok([
                device.texture(w, h, 1, &dx::glint_pixels(pixels, false, bake))?,
                device.texture(w, h, 1, &dx::glint_pixels(pixels, true, bake))?,
            ])
        };
        Ok(Self {
            item: make(item)?,
            armor: make(armor)?,
        })
    }
    pub fn texture(&self, kind: u8, tint: u32) -> &dx::Texture {
        &(if kind == 2 { &self.armor } else { &self.item })[usize::from(tint != 0)]
    }
    pub fn bytes(&self) -> u64 {
        self.item.iter().chain(&self.armor).map(|t| t.bytes).sum()
    }
}
impl Renderer {
    pub(super) fn draw_glints(
        &mut self,
        batches: &[frame::Batch],
        tint: u32,
        stage: bool,
    ) -> Result<()> {
        if batches.is_empty() {
            return Ok(());
        }
        self.hud.atlas.prepare(&mut self.device)?;
        let textures = self.hud.atlas.textures();
        let mut stream = None;
        if stage && batches.len() > 1 {
            match stage_glint_batches(&mut self.device, batches, &textures) {
                Ok(Some((staged, draws))) => {
                    self.entity_stage_stats[2] += 1;
                    self.entity_stage_stats[3] += draws as u64 - 1;
                    stream = Some(staged);
                }
                Ok(None) => {}
                Err(error) if dx::is_device_lost(error.as_ref()) => return Err(error),
                Err(error) => {
                    if !self.entity_stage_warned {
                        log::warn!(
                            "D3D9 glint staging failed ({error}); keeping per-batch uploads"
                        );
                        self.entity_stage_warned = true;
                    }
                }
            }
        }
        let mut first = 0;
        for b in batches {
            let Some(mask) = &textures[b.page] else {
                continue;
            };
            self.device
                .state(b.state, mask, Some(self.glint.texture(b.state.glint, tint)))?;
            if let Some(stream) = &stream {
                let end = first + b.vertices.len() as u32;
                self.device.draw_staged_quads(stream, first..end)?;
                first = end;
            } else {
                self.device.draw_transient(&b.vertices)?;
            }
        }
        Ok(())
    }
}

fn stage_glint_batches(
    device: &mut dx::Device,
    batches: &[frame::Batch],
    masks: &[Option<dx::Texture>],
) -> Result<Option<(dx::StagedQuads, usize)>> {
    let mut slices = smallvec::SmallVec::<[&[dx::Vertex]; 8]>::new();
    let mut remaining = device.max_vertices();
    for b in batches {
        if masks[b.page].is_none() || b.vertices.is_empty() {
            continue;
        }
        if b.vertices.len() > remaining || slices.try_reserve(1).is_err() {
            return Ok(None);
        }
        remaining -= b.vertices.len();
        slices.push(b.vertices.as_slice());
    }
    if slices.len() < 2 {
        return Ok(None);
    }
    Ok(Some((device.stage_quad_slices(&slices)?, slices.len())))
}
