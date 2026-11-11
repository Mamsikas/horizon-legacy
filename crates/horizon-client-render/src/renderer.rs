//! The gameplay-facing renderer. A legacy executable selects DX9 at build time;
//! the ordinary client's backend, defaults and GPU implementation are unchanged.
#[cfg(all(windows, feature = "legacy-d3d9"))]
mod legacy;
#[cfg(not(all(windows, feature = "legacy-d3d9")))]
pub use crate::gfx::Renderer;
#[cfg(all(windows, feature = "legacy-d3d9"))]
pub use legacy::Renderer;
