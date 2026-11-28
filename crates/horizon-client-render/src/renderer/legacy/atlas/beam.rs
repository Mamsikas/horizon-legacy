use super::*;

// The beacon and gateway repeat a sprite in V, while the virtual atlas
// places unrelated sprites on either side of it. Give these materials an
// isolated texture so a tall beam needs only its original four wall quads.
const MAX_SPRITES: usize = 4;
const MAX_SIDE: u32 = 256;
const FIRST_PAGE: usize = ENTITY_PAGE - MAX_SPRITES;

#[derive(Default)]
pub(super) struct Sprites {
    entries: Vec<Sprite>,
}

struct Sprite {
    rect: [u32; 4],
    texture: Option<dx::Texture>,
    dirty: bool,
}

impl Sprites {
    pub(super) fn uses_page(page: usize) -> bool {
        (FIRST_PAGE..ENTITY_PAGE).contains(&page)
    }

    pub(super) fn get(&self, page: usize) -> Option<&dx::Texture> {
        self.entries.get(page - FIRST_PAGE)?.texture.as_ref()
    }

    pub(super) fn bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|sprite| sprite.texture.as_ref())
            .map(|texture| texture.bytes)
            .sum()
    }

    pub(super) fn dirty(&mut self, [x, y, w, h]: [u32; 4]) {
        for sprite in &mut self.entries {
            let [sx, sy, sw, sh] = sprite.rect;
            sprite.dirty |= x < sx + sw && sx < x + w && y < sy + sh && sy < y + h;
        }
    }

    pub(super) fn prepare(
        &mut self,
        image: &dx::TileImage,
        device: &mut dx::Device,
        rect: [f32; 4],
    ) -> Result<Option<(usize, u32)>> {
        let [u0, v0, u1, v_height] = rect;
        let rect = [
            (u0 * image.width as f32).round() as u32,
            (v0 * image.height as f32).round() as u32,
            ((u1 - u0) * image.width as f32).round() as u32,
            (v_height * image.height as f32).round() as u32,
        ];
        let [x, y, w, h] = rect;
        if w == 0
            || h == 0
            || w.max(h) > MAX_SIDE.min(device.caps.max_texture)
            || x.checked_add(w).is_none_or(|end| end > image.width)
            || y.checked_add(h).is_none_or(|end| end > image.height)
        {
            return Ok(None);
        }
        let Some(limit) = device.repeat_v_limit(w, h) else {
            return Ok(None);
        };
        let index = match self.entries.iter().position(|entry| entry.rect == rect) {
            Some(index) => index,
            None if self.entries.len() < MAX_SPRITES => {
                let index = self.entries.len();
                self.entries.push(Sprite {
                    rect,
                    texture: None,
                    dirty: true,
                });
                index
            }
            None => return Ok(None),
        };
        let sprite = &mut self.entries[index];
        if sprite.dirty {
            let result = (|| device.texture(w, h, 1, &image.copy_rect(rect)?))();
            match result {
                Ok(texture) => sprite.texture = Some(texture),
                Err(err) if dx::is_device_lost(err.as_ref()) => return Err(err),
                Err(err) => {
                    log::warn!("D3D9 beam texture allocation failed ({err}); repeating atlas UVs");
                    sprite.texture = None;
                }
            }
            sprite.dirty = false;
        }
        Ok(sprite.texture.as_ref().map(|_| (FIRST_PAGE + index, limit)))
    }
}
