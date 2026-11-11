//! Safe, render-thread-only Direct3D 9 HAL ownership.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(windows)]
mod device;
#[cfg(windows)]
pub use device::*;

mod atlas;
pub use atlas::{AtlasLayout, AtlasSet};

#[cfg(windows)]
pub mod clipboard;
