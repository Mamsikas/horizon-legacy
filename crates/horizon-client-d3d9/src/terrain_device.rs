//! Native objects for the optional hardware-only SM2 vertex path.
use windows::Win32::Graphics::Direct3D9::*;
#[derive(Clone)]
pub(super) struct TerrainPipeline {
    pub shader: IDirect3DVertexShader9,
    pub compact: IDirect3DVertexDeclaration9,
    pub wide: IDirect3DVertexDeclaration9,
}
impl TerrainPipeline {
    pub fn new(device: &IDirect3DDevice9) -> windows::core::Result<Self> {
        let element = |offset, kind: D3DDECLTYPE, usage: D3DDECLUSAGE, index| D3DVERTEXELEMENT9 {
            Stream: 0,
            Offset: offset,
            Type: kind.0 as u8,
            Method: D3DDECLMETHOD_DEFAULT.0 as u8,
            Usage: usage.0 as u8,
            UsageIndex: index,
        };
        let end = D3DVERTEXELEMENT9 {
            Stream: 0xff,
            Offset: 0,
            Type: D3DDECLTYPE_UNUSED.0 as u8,
            Method: 0,
            Usage: 0,
            UsageIndex: 0,
        };
        let compact = [
            element(0, D3DDECLTYPE_SHORT4, D3DDECLUSAGE_POSITION, 0),
            element(8, D3DDECLTYPE_D3DCOLOR, D3DDECLUSAGE_COLOR, 0),
            element(12, D3DDECLTYPE_SHORT2, D3DDECLUSAGE_TEXCOORD, 0),
            element(16, D3DDECLTYPE_SHORT2, D3DDECLUSAGE_TEXCOORD, 1),
            end,
        ];
        let wide = [
            element(0, D3DDECLTYPE_FLOAT3, D3DDECLUSAGE_POSITION, 0),
            element(12, D3DDECLTYPE_D3DCOLOR, D3DDECLUSAGE_COLOR, 0),
            element(16, D3DDECLTYPE_FLOAT2, D3DDECLUSAGE_TEXCOORD, 0),
            element(24, D3DDECLTYPE_SHORT2, D3DDECLUSAGE_TEXCOORD, 1),
            end,
        ];
        // SAFETY: terminated SDK-generated VS2 and declarations matching the
        // checked 20/28-byte CPU encodings. The driver validates support.
        unsafe {
            Ok(Self {
                shader: device.CreateVertexShader(crate::terrain::bytecode().as_ptr())?,
                compact: device.CreateVertexDeclaration(compact.as_ptr())?,
                wide: device.CreateVertexDeclaration(wide.as_ptr())?,
            })
        }
    }
}
