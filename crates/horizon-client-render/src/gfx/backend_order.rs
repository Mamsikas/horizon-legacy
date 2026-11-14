use crate::graphics::GraphicsApi;

/// The backends to open a device on, each tried in turn until one gives a
/// device. `api` is the player's choice (`GraphicsApi::with_env` applied);
/// `os` is `std::env::consts::OS`; `arch` the machine's (`horizon_system::machine_arch`), not the
/// build's; `wgpu_backend`: whether `WGPU_BACKEND` is set, which picks the backends itself.
/// Auto tries DirectX 12 first on Windows ARM64, the x64 build emulated there included,
/// then Vulkan, based on the device losses seen there (#7513). Other
/// Windows systems try Vulkan, then DirectX 12, then OpenGL; Linux tries Vulkan then
/// OpenGL. Each is tried when the one before has no surface, adapter or device. A native
/// crash in the driver can't be
/// caught in the game; the launcher switches the setting to DirectX 12 after one at
/// the renderer's start (#746). macOS has Metal only.
pub(super) fn backend_order(
    api: GraphicsApi,
    os: &str,
    arch: &str,
    wgpu_backend: bool,
) -> Vec<wgpu::Backends> {
    use wgpu::Backends;
    if os == "macos" || wgpu_backend {
        return vec![Backends::default()];
    }
    let windows = os == "windows";
    match api {
        GraphicsApi::Auto if windows && arch == "aarch64" => {
            vec![Backends::DX12, Backends::VULKAN, Backends::GL]
        }
        GraphicsApi::Auto if windows => vec![Backends::VULKAN, Backends::DX12, Backends::GL],
        GraphicsApi::Auto => vec![Backends::VULKAN, Backends::GL],
        GraphicsApi::Dx12 if windows => vec![Backends::DX12],
        GraphicsApi::Vulkan | GraphicsApi::Dx12 => vec![Backends::VULKAN],
        GraphicsApi::OpenGl => vec![Backends::GL],
        #[cfg(all(windows, feature = "legacy-d3d9"))]
        GraphicsApi::LegacyD3d9 => Vec::new(),
    }
}
