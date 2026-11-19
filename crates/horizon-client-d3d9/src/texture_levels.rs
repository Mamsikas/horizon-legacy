//! D3D9 has no mip-range texture views. Recreate only when the option changes;
//! retain a compressed copy of the small tail, and copy level zero directly.
use super::*;

pub(super) struct Backup {
    levels: u32,
    tail: Vec<u8>,
}
impl Texture {
    pub fn mip_cpu_bytes(&self) -> usize {
        self.mip_backup.as_ref().map_or(0, |b| b.tail.capacity())
    }
}
impl Device {
    pub fn limit_texture_levels(&mut self, texture: &mut Texture, levels: u32) -> Result<()> {
        if !Rc::ptr_eq(&texture.owner, &self.inner) {
            return Err("D3D9 mip texture belongs to another device".into());
        }
        let original = texture
            .mip_backup
            .as_ref()
            .map_or(texture.levels, |b| b.levels);
        let levels = levels.clamp(1, original);
        if levels == texture.levels {
            return Ok(());
        }
        if texture.mip_backup.is_none() {
            let mut tail = Vec::new();
            for level in 1..original {
                read(
                    &texture.raw,
                    level,
                    (texture.width >> level).max(1),
                    (texture.height >> level).max(1),
                    &mut tail,
                )?;
            }
            texture.mip_backup = Some(Backup {
                levels: original,
                tail: lz4_flex::block::compress_prepend_size(&tail),
            });
        }
        let tail =
            lz4_flex::block::decompress_size_prepended(&texture.mip_backup.as_ref().unwrap().tail)?;
        let mut raw = None;
        // SAFETY: same adapter/format/dimensions as the existing MANAGED texture, fewer or restored levels.
        unsafe {
            self.inner.device.CreateTexture(
                texture.width,
                texture.height,
                levels,
                0,
                D3DFMT_A8R8G8B8,
                D3DPOOL_MANAGED,
                &mut raw,
                ptr::null_mut(),
            )?;
        }
        let raw = raw.ok_or("D3D9 replacement mip texture missing")?;
        copy_base(&texture.raw, &raw, texture.width, texture.height)?;
        let mut at = 0;
        for level in 1..levels {
            let w = (texture.width >> level).max(1);
            let h = (texture.height >> level).max(1);
            let end = at + w as usize * h as usize * 4;
            write(
                &raw,
                level,
                w,
                h,
                tail.get(at..end)
                    .ok_or("D3D9 retained mip tail length mismatch")?,
            )?;
            at = end;
        }
        let base = raw.cast::<IDirect3DBaseTexture9>()?;
        texture.raw = raw;
        texture.base = base;
        texture.levels = levels;
        texture.bytes = (0..levels)
            .map(|l| {
                u64::from((texture.width >> l).max(1)) * u64::from((texture.height >> l).max(1)) * 4
            })
            .sum();
        if levels == original {
            texture.mip_backup = None;
        }
        self.cached_state = None;
        Ok(())
    }
}
fn valid(lock: &D3DLOCKED_RECT, width: u32) -> bool {
    !lock.pBits.is_null() && lock.Pitch >= (width * 4) as i32
}
fn read(
    texture: &IDirect3DTexture9,
    level: u32,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<()> {
    let mut lock = D3DLOCKED_RECT::default();
    // SAFETY: a bounded owned MANAGED mip; copy only its declared rows and release the lock on every path.
    unsafe {
        texture.LockRect(level, &mut lock, ptr::null(), D3DLOCK_READONLY as u32)?;
        if !valid(&lock, width) {
            texture.UnlockRect(level)?;
            return Err("D3D9 retained mip lock invalid".into());
        }
        for y in 0..height as usize {
            out.extend_from_slice(std::slice::from_raw_parts(
                lock.pBits.cast::<u8>().add(y * lock.Pitch as usize),
                width as usize * 4,
            ));
        }
        texture.UnlockRect(level)?;
    }
    Ok(())
}
fn write(
    texture: &IDirect3DTexture9,
    level: u32,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<()> {
    let mut lock = D3DLOCKED_RECT::default();
    // SAFETY: pixels is exactly this mip's raw BGRA rows; no pointer escapes the lock.
    unsafe {
        texture.LockRect(level, &mut lock, ptr::null(), 0)?;
        if !valid(&lock, width) {
            texture.UnlockRect(level)?;
            return Err("D3D9 replacement mip lock invalid".into());
        }
        for y in 0..height as usize {
            ptr::copy_nonoverlapping(
                pixels[y * width as usize * 4..].as_ptr(),
                lock.pBits.cast::<u8>().add(y * lock.Pitch as usize),
                width as usize * 4,
            );
        }
        texture.UnlockRect(level)?;
    }
    Ok(())
}
fn copy_base(
    source: &IDirect3DTexture9,
    target: &IDirect3DTexture9,
    width: u32,
    height: u32,
) -> Result<()> {
    let (mut src, mut dst) = (D3DLOCKED_RECT::default(), D3DLOCKED_RECT::default());
    // SAFETY: distinct MANAGED textures of the same dimensions; both locks are released even on failure.
    unsafe {
        source.LockRect(0, &mut src, ptr::null(), D3DLOCK_READONLY as u32)?;
        if let Err(error) = target.LockRect(0, &mut dst, ptr::null(), 0) {
            source.UnlockRect(0)?;
            return Err(error.into());
        }
        let valid = valid(&src, width) && valid(&dst, width);
        if valid {
            for y in 0..height as usize {
                ptr::copy_nonoverlapping(
                    src.pBits.cast::<u8>().add(y * src.Pitch as usize),
                    dst.pBits.cast::<u8>().add(y * dst.Pitch as usize),
                    width as usize * 4,
                );
            }
        }
        let (a, b) = (source.UnlockRect(0), target.UnlockRect(0));
        a?;
        b?;
        if !valid {
            return Err("D3D9 base mip copy lock invalid".into());
        }
    }
    Ok(())
}
