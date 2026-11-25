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
    pub(super) fn draw_glints(&mut self, batches: &[frame::Batch], tint: u32) -> Result<()> {
        if batches.is_empty() {
            return Ok(());
        }
        self.hud.atlas.prepare(&mut self.device)?;
        let textures = self.hud.atlas.textures();
        for b in batches {
            let Some(mask) = &textures[b.page] else {
                continue;
            };
            self.device
                .state(b.state, mask, Some(self.glint.texture(b.state.glint, tint)))?;
            self.device.draw_transient(&b.vertices)?;
        }
        Ok(())
    }
}
