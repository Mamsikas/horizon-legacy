//! A separate bounded XYZRHW stream: CPU-projected GUI needs no software T&L.
use super::*;
use crate::ScreenVertex;
const SCREEN_FVF: u32 = D3DFVF_XYZRHW | D3DFVF_DIFFUSE | D3DFVF_TEX1;

#[derive(Default)]
pub(super) struct Stream {
    raw: Option<IDirect3DVertexBuffer9>,
    ring: Ring,
    pub(super) serial: u64,
}
pub struct ScreenTriangles {
    owner: Rc<Inner>,
    serial: u64,
    first: u32,
    count: u32,
}
impl Device {
    pub fn stage_screen(&mut self, vertices: &[ScreenVertex]) -> Result<ScreenTriangles> {
        if vertices.is_empty() || vertices.len() % 3 != 0 || vertices.len() > self.max_vertices() {
            return Err("D3D9 screen upload exceeds complete triangle/caps limits".into());
        }
        let capacity = self.max_vertices() * size_of::<ScreenVertex>();
        if self.screen.raw.is_none() {
            // SAFETY: checked capacity and FVF matching ScreenVertex's repr(C).
            // These vertices are already transformed, so DEFAULT is GPU input;
            // SOFTWAREPROCESSING is deliberately absent, even on a SWVP HAL.
            unsafe {
                self.inner.device.CreateVertexBuffer(
                    capacity as u32,
                    (D3DUSAGE_DYNAMIC | D3DUSAGE_WRITEONLY) as u32,
                    SCREEN_FVF,
                    D3DPOOL_DEFAULT,
                    &mut self.screen.raw,
                    ptr::null_mut(),
                )?;
            }
        }
        let bytes = bytemuck::cast_slice::<_, u8>(vertices);
        let (offset, discard) = self
            .screen
            .ring
            .reserve(bytes.len(), capacity)
            .ok_or("D3D9 screen stream overflow")?;
        let serial = self
            .inner
            .stream_sequence
            .get()
            .checked_add(1)
            .ok_or("D3D9 upload sequence exhausted")?;
        let buffer = self
            .screen
            .raw
            .as_ref()
            .ok_or("D3D9 missing screen buffer")?;
        // SAFETY: bounded lock/copy; NOOVERWRITE appends and DISCARD renames
        // on wrap. No locked memory or raw COM resource escapes this function.
        unsafe {
            let mut memory = ptr::null_mut();
            buffer.Lock(
                offset as u32,
                bytes.len() as u32,
                &mut memory,
                if discard {
                    D3DLOCK_DISCARD
                } else {
                    D3DLOCK_NOOVERWRITE
                } as u32,
            )?;
            if memory.is_null() {
                buffer.Unlock()?;
                return Err("D3D9 empty screen lock".into());
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            buffer.Unlock()?;
        }
        self.inner.stream_sequence.set(serial);
        self.screen.serial = serial;
        self.uploads += bytes.len() as u64;
        self.stream_uploads += bytes.len() as u64;
        self.stream_locks += 1;
        Ok(ScreenTriangles {
            owner: self.inner.clone(),
            serial,
            first: (offset / size_of::<ScreenVertex>()) as u32,
            count: vertices.len() as u32,
        })
    }
    pub fn draw_screen(
        &mut self,
        vertices: &ScreenTriangles,
        range: std::ops::Range<u32>,
    ) -> Result<()> {
        if !Rc::ptr_eq(&vertices.owner, &self.inner)
            || vertices.serial != self.screen.serial
            || range.start > range.end
            || range.end > vertices.count
            || range.start % 3 != 0
            || range.end % 3 != 0
        {
            return Err("D3D9 stale or invalid screen triangle range".into());
        }
        if range.is_empty() {
            return Ok(());
        }
        let buffer = self
            .screen
            .raw
            .as_ref()
            .ok_or("D3D9 missing screen stream")?;
        // SAFETY: ownership/generation/range were checked; world draws restore
        // their FVF/shader through vertex_pipeline before using their streams.
        unsafe {
            if !self.screen_active {
                self.inner.device.SetVertexShader(None)?;
                self.inner.device.SetFVF(SCREEN_FVF)?;
                self.terrain_encoding = None;
                self.screen_active = true;
            }
            self.inner
                .device
                .SetStreamSource(0, buffer, 0, size_of::<ScreenVertex>() as u32)?;
            self.inner.device.DrawPrimitive(
                D3DPT_TRIANGLELIST,
                vertices.first + range.start,
                range.len() as u32 / 3,
            )?;
        }
        self.draws += 1;
        Ok(())
    }
    pub fn draw_screen_triangles(&mut self, vertices: &[ScreenVertex]) -> Result<()> {
        let limit = self.max_vertices() / 3 * 3;
        for chunk in vertices.chunks(limit) {
            let staged = self.stage_screen(chunk)?;
            self.draw_screen(&staged, 0..chunk.len() as u32)?;
        }
        Ok(())
    }
}
