//! Backbuffer rows to opaque RGBA. No readback occurs in the ordinary frame path.
pub(crate) fn rgba_row(source: &[u8], target: &mut [u8], rgb565: bool) {
    if rgb565 {
        for (pixel, out) in source
            .as_chunks::<2>()
            .0
            .iter()
            .zip(target.as_chunks_mut::<4>().0)
        {
            let value = u16::from_le_bytes(*pixel);
            let r = (value >> 11) as u8;
            let g = ((value >> 5) & 63) as u8;
            let b = (value & 31) as u8;
            *out = [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2, 255];
        }
    } else {
        for (pixel, out) in source
            .as_chunks::<4>()
            .0
            .iter()
            .zip(target.as_chunks_mut::<4>().0)
        {
            *out = [pixel[2], pixel[1], pixel[0], 255];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readback_preserves_both_color_formats() {
        let mut output = [0; 24];
        rgba_row(
            &[0, 0, 0, 248, 224, 7, 31, 0, 255, 255, 16, 132],
            &mut output,
            true,
        );
        assert_eq!(
            output,
            [
                0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                132, 130, 132, 255
            ]
        );
        rgba_row(&[3, 2, 1, 0], &mut output[..4], false);
        assert_eq!(&output[..4], &[1, 2, 3, 255]);
    }
}
