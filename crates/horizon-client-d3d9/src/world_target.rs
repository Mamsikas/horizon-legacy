//! Resolution scale uses native RT/depth surfaces and one hardware StretchRect before the HUD.
use super::*;

struct Target {
    size: [u32; 2],
    color: IDirect3DSurface9,
    depth: IDirect3DSurface9,
}
#[derive(Default)]
pub(super) struct State {
    target: Option<Target>,
    refused: Option<[u32; 2]>,
    pub(super) active: bool,
}
impl Device {
    pub fn world_target_bytes(&self) -> u64 {
        self.inner.world.borrow().target.as_ref().map_or(0, |t| {
            u64::from(t.size[0])
                * u64::from(t.size[1])
                * (u64::from(self.caps.color_bits / 8)
                    + if self.present.AutoDepthStencilFormat == D3DFMT_D16 {
                        2
                    } else {
                        4
                    })
        })
    }
    pub(super) fn select_world_target(&mut self, scale: f32) -> Result<()> {
        let Some(size) = crate::world_buffer_size(
            self.present.BackBufferWidth,
            self.present.BackBufferHeight,
            scale,
            self.caps.max_texture,
        ) else {
            let mut world = self.inner.world.borrow_mut();
            if world.target.is_some() || world.refused.is_some() {
                *world = State::default();
            }
            return Ok(());
        };
        if self.inner.world.borrow().refused == Some(size) {
            return Ok(());
        }
        let result = (|| -> Result<()> {
            let mut world = self.inner.world.borrow_mut();
            if world.target.as_ref().is_none_or(|t| t.size != size) {
                let (mut color, mut depth) = (None, None);
                // SAFETY: matching bounded dimensions, formats and sample count on this device.
                unsafe {
                    self.inner.device.CreateRenderTarget(
                        size[0],
                        size[1],
                        self.present.BackBufferFormat,
                        D3DMULTISAMPLE_NONE,
                        0,
                        false,
                        &mut color,
                        ptr::null_mut(),
                    )?;
                    self.inner.device.CreateDepthStencilSurface(
                        size[0],
                        size[1],
                        self.present.AutoDepthStencilFormat,
                        D3DMULTISAMPLE_NONE,
                        0,
                        true,
                        &mut depth,
                        ptr::null_mut(),
                    )?;
                }
                world.target = Some(Target {
                    size,
                    color: color.ok_or("D3D9 scaled color target missing")?,
                    depth: depth.ok_or("D3D9 scaled depth target missing")?,
                });
                log::info!(
                    "D3D9 world target {}x{}; HUD/presentation {}x{}",
                    size[0],
                    size[1],
                    self.present.BackBufferWidth,
                    self.present.BackBufferHeight
                );
            }
            let target = world.target.as_ref().unwrap();
            // SAFETY: detach depth before changing RT size, then bind its matching depth and viewport.
            unsafe {
                self.inner.device.SetDepthStencilSurface(None)?;
                self.inner.device.SetRenderTarget(0, &target.color)?;
                self.inner.device.SetDepthStencilSurface(&target.depth)?;
                self.inner.device.SetViewport(&D3DVIEWPORT9 {
                    Width: size[0],
                    Height: size[1],
                    MaxZ: 1.0,
                    ..Default::default()
                })?;
            }
            world.active = true;
            Ok(())
        })();
        if let Err(error) = result {
            if is_device_lost(error.as_ref()) {
                return Err(error);
            }
            self.restore_main_target()?;
            *self.inner.world.borrow_mut() = State {
                refused: Some(size),
                ..Default::default()
            };
            log::warn!(
                "D3D9 resolution scale unavailable for {size:?}: {error}; keeping native resolution and the saved setting"
            );
        }
        Ok(())
    }
    pub(super) fn restore_main_target(&self) -> Result<()> {
        let presentation = self.inner.presentation.borrow();
        let p = presentation
            .as_ref()
            .ok_or("D3D9 presentation surface missing")?;
        // SAFETY: main RT, depth and viewport have identical window dimensions and sample count.
        unsafe {
            let main = p.chain.GetBackBuffer(0, D3DBACKBUFFER_TYPE_MONO)?;
            self.inner.device.SetDepthStencilSurface(None)?;
            self.inner.device.SetRenderTarget(0, &main)?;
            self.inner.device.SetDepthStencilSurface(&p._depth)?;
            self.inner.device.SetViewport(&D3DVIEWPORT9 {
                Width: p.width,
                Height: p.height,
                MaxZ: 1.0,
                ..Default::default()
            })?;
        }
        Ok(())
    }
    pub fn finish_world(&mut self) -> Result<bool> {
        if !self.inner.world.borrow().active {
            return Ok(false);
        }
        // StretchRect is forbidden inside BeginScene/EndScene. Never read back to the CPU.
        unsafe {
            self.inner.device.EndScene()?;
        }
        self.scene = false;
        self.restore_main_target()?;
        let result = {
            let world = self.inner.world.borrow();
            let target = world.target.as_ref().unwrap();
            let filters = (D3DPTFILTERCAPS_MINFLINEAR | D3DPTFILTERCAPS_MAGFLINEAR) as u32;
            let filter = if self.caps.raw.StretchRectFilterCaps & filters == filters {
                D3DTEXF_LINEAR
            } else {
                D3DTEXF_NONE
            };
            // SAFETY: distinct DEFAULT render targets with equal color formats, outside a scene.
            unsafe {
                self.inner.device.StretchRect(
                    &target.color,
                    ptr::null(),
                    &self
                        .inner
                        .presentation
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .chain
                        .GetBackBuffer(0, D3DBACKBUFFER_TYPE_MONO)?,
                    ptr::null(),
                    filter,
                )
            }
        };
        self.inner.world.borrow_mut().active = false;
        if let Err(error) = result {
            if error.code() == D3DERR_DEVICELOST || error.code() == D3DERR_DEVICENOTRESET {
                return Err(error.into());
            }
            let mut world = self.inner.world.borrow_mut();
            world.refused = world.target.as_ref().map(|t| t.size);
            world.target = None;
            log::warn!(
                "D3D9 scaled resolve unavailable: {error}; native resolution resumes next frame"
            );
        }
        // SAFETY: the previous scene ended; GUI follows at full output resolution.
        unsafe {
            self.inner.device.BeginScene()?;
        }
        self.scene = true;
        self.cached_state = None;
        Ok(true)
    }
}
