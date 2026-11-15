//! Probe native windowed color conversion and matching depth, without allocating surfaces.
use super::*;

pub(super) fn choose(
    d3d: &IDirect3D9,
    desktop: D3DFORMAT,
    color: crate::ColorDepth,
) -> Result<(D3DFORMAT, D3DFORMAT)> {
    let colors: &[D3DFORMAT] = match color {
        crate::ColorDepth::Bits16 => &[D3DFMT_R5G6B5, D3DFMT_X8R8G8B8, D3DFMT_A8R8G8B8],
        crate::ColorDepth::Bits32 => &[D3DFMT_X8R8G8B8, D3DFMT_A8R8G8B8],
    };
    // Keep D24 precision ahead of the color preference. RGB565 must not silently
    // introduce a D16 precision regression in distant terrain or entity overlays.
    for depth in [D3DFMT_D24X8, D3DFMT_D16] {
        for &backbuffer in colors {
            // SAFETY: read-only adapter capability probes; no user pointers escape.
            let supported = unsafe {
                d3d.CheckDeviceType(0, D3DDEVTYPE_HAL, desktop, backbuffer, BOOL(1))
                    .is_ok()
                    && d3d
                        .CheckDeviceFormat(
                            0,
                            D3DDEVTYPE_HAL,
                            desktop,
                            D3DUSAGE_DEPTHSTENCIL as u32,
                            D3DRTYPE_SURFACE,
                            depth,
                        )
                        .is_ok()
                    && d3d
                        .CheckDepthStencilMatch(0, D3DDEVTYPE_HAL, desktop, backbuffer, depth)
                        .is_ok()
            };
            if supported {
                return Ok((backbuffer, depth));
            }
        }
    }
    Err("D3D9 HAL: no compatible windowed color/depth pair".into())
}
