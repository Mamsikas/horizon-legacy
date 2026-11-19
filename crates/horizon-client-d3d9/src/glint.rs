//! Fixed-function glint inputs: preserve masks, repeat the supplied scrolling UV,
//! and square the sampled colour (or bake the square on two-stage adapters).
pub fn glint_pixels(pixels: &[u8], monochrome: bool, bake_square: bool) -> Vec<u8> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let brightest = *p[..3].iter().max().unwrap();
            let rgb: [u8; 3] = std::array::from_fn(|i| {
                let c = if monochrome { brightest } else { p[i] };
                if bake_square {
                    (u32::from(c) * u32::from(c) / 255) as u8
                } else {
                    c
                }
            });
            [rgb[0], rgb[1], rgb[2], 255]
        })
        .collect()
}
pub fn glint_color(strength: f32, tint: u32, alpha: u8) -> u32 {
    let strength = strength.clamp(0.0, 1.0);
    let rgb: [u8; 3] = std::array::from_fn(|i| {
        let c = if tint == 0 {
            1.0
        } else {
            (tint >> (i * 8) & 255) as f32 / 255.0
        } * strength;
        (c * c * 255.0).round() as u8
    });
    u32::from(alpha) << 24 | u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2])
}
#[derive(Clone, Copy)]
pub struct ArmorGlint {
    scroll: [f32; 2],
}
impl ArmorGlint {
    /// One scrolling transform for the frame, not a remainder/trig per vertex.
    pub fn new(ticks: f32) -> Self {
        Self {
            scroll: [-(ticks % 110000.0) / 110000.0, (ticks % 30000.0) / 30000.0],
        }
    }
    pub fn uv(self, uv: [f32; 2]) -> [f32; 2] {
        // sin/cos of the shader's fixed ten-degree rotation.
        let (s, c) = (0.17364818, 0.9848077);
        let [x, y] = uv.map(|v| v * 0.16);
        [
            x * c - y * s + self.scroll[0],
            x * s + y * c + self.scroll[1],
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glint_keeps_mask_alpha_separate_and_matches_squared_strength_and_scroll() {
        assert_eq!(
            glint_pixels(&[64, 128, 255, 0], false, false),
            [64, 128, 255, 255]
        );
        assert_eq!(
            glint_pixels(&[64, 128, 255, 9], true, true),
            [255, 255, 255, 255]
        );
        assert_eq!(
            glint_pixels(&[0, 128, 255, 0], false, true),
            [0, 64, 255, 255]
        );
        assert_eq!(glint_color(0.0, 0, 123), 123 << 24);
        assert_eq!(glint_color(1.0, 0, 123), 0x7bffffff);
        assert_eq!(glint_color(0.5, 0x000000ff, 255), 0xff400000);
        assert_eq!(ArmorGlint::new(0.0).uv([0.0; 2]), [0.0; 2]);
        let rotated = ArmorGlint::new(0.0).uv([6.25, 0.0]);
        assert!((rotated[0] - 0.9848077).abs() < 1e-6);
        assert!((rotated[1] - 0.17364818).abs() < 1e-6);
        let moved = ArmorGlint::new(15000.0).uv([0.0; 2]);
        assert!((moved[0] + 15000.0 / 110000.0).abs() < 1e-6);
        assert!((moved[1] - 0.5).abs() < 1e-6);
    }
}
