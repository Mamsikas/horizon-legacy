//! Safe, render-thread-only Direct3D 9 HAL ownership.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(windows)]
mod device;
#[cfg(windows)]
pub use device::*;

mod atlas;
pub use atlas::{AtlasLayout, AtlasSet, AtlasUv};

#[cfg(windows)]
pub mod clipboard;

mod geometry;
pub use geometry::{IndexBatch, MeshData, PaintWindow, Ring, Vertex, fog_interval};
mod screen;
pub use screen::{GuiDepth, ScreenVertex};
mod fog;
pub use fog::Fog;
mod tiles;
pub use tiles::TileImage;
mod terrain;
pub use terrain::{EncodedVertices, VertexEncoding};
mod animation;
pub use animation::AnimationCache;
mod upload_batch;
pub use upload_batch::{UploadBatch, UploadPage, UploadPart};
mod texture_pages;
pub use texture_pages::{PageOrder, TextureRange, append_texture_pages};
