//! Safe, render-thread-only Direct3D 9 HAL ownership.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(windows)]
mod device;
#[cfg(windows)]
pub use device::*;
#[cfg(windows)]
mod thread_time;
#[cfg(windows)]
pub use thread_time::{process_cpu_time, system_cpu_ticks, thread_cpu_time};

mod atlas;
pub use atlas::{AtlasLayout, AtlasSet, AtlasUv};

#[cfg(windows)]
pub mod clipboard;

mod geometry;
pub use geometry::{IndexBatch, MeshData, PaintWindow, Ring, Vertex, fog_interval};
mod preferences;
pub use preferences::{ColorDepth, Preferences, VertexProcessing};
#[cfg(any(windows, test))]
mod readback;
mod screen;
pub use screen::{GuiDepth, ScreenVertex};
mod fog;
pub use fog::{Fog, FogProgram};
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
