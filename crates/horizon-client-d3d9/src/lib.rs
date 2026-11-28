//! Safe, render-thread-only Direct3D 9 HAL ownership.
#![deny(unsafe_op_in_unsafe_fn)]
pub const fn development_build() -> bool {
    cfg!(feature = "dev-diagnostics")
}
#[cfg(feature = "dev-diagnostics")]
pub mod benchmark;
#[cfg(any(windows, test))]
mod stream_cache;
// Pass markers compile to empty inline methods in ordinary Windows builds.
#[cfg(any(windows, all(test, feature = "submit-profile")))]
mod submit_profile;
#[cfg(windows)]
pub use submit_profile::SubmitPass;
#[cfg(windows)]
mod device;
#[cfg(windows)]
pub use device::*;
#[cfg(windows)]
mod thread_time;
#[cfg(windows)]
pub use thread_time::{system_cpu_ticks, thread_cpu_time};

mod atlas;
pub use atlas::{AtlasLayout, AtlasSet, AtlasUv};

#[cfg(windows)]
pub mod clipboard;

mod geometry;
pub use geometry::{IndexBatch, IndexSavings, MeshData, PaintWindow, Ring, Vertex, fog_interval};
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
mod shadow_queue;
pub use shadow_queue::ShadowQueue;
mod world_size;
pub use world_size::world_buffer_size;

mod glint;
pub use glint::{ArmorGlint, glint_color, glint_pixels};

/// No settings file is read or written. Explicit runtime values take precedence.
pub fn log_stats_enabled() -> bool {
    std::env::var_os("HORIZON_D3D9_LOG_STATS")
        .map_or(cfg!(feature = "dev-diagnostics"), |value| value == "1")
}
