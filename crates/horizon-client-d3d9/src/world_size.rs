//! Match Horizon's manual world scale while bounding native target dimensions.
pub fn world_buffer_size(width: u32, height: u32, scale: f32, maximum: u32) -> Option<[u32; 2]> {
    let width = width.max(1);
    let height = height.max(1);
    let scale = if scale.is_finite() {
        scale.clamp(0.25, 2.0)
    } else {
        1.0
    };
    if scale == 1.0 {
        return None;
    }
    let scale = scale.min(maximum.max(1) as f32 / width.max(height) as f32);
    let size = [width, height].map(|n| ((n as f32 * scale).round() as u32).max(1));
    (size != [width, height]).then_some(size)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_scale_preserves_output_size_and_caps_the_offscreen_target() {
        assert_eq!(world_buffer_size(1024, 600, 1.0, 2048), None);
        assert_eq!(world_buffer_size(1024, 600, 0.5, 2048), Some([512, 300]));
        assert_eq!(world_buffer_size(1024, 600, 2.0, 2048), Some([2048, 1200]));
        assert_eq!(world_buffer_size(1920, 1080, 2.0, 2048), Some([2048, 1152]));
        assert_eq!(world_buffer_size(854, 480, 0.0, 2048), Some([214, 120]));
        assert_eq!(world_buffer_size(1024, 600, f32::NAN, 2048), None);
        assert_eq!(world_buffer_size(0, 0, 0.25, 2048), None);
    }
}
