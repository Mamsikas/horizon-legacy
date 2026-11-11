//! Standalone device/resize/depth/alpha smoke test. This is not the Horizon game.
#[cfg(windows)]
mod windows_smoke {
    use horizon_client_d3d9::{Blend, Device, DrawState, Result, Texture, Vertex};
    use std::{
        fs::File,
        io::Write,
        sync::{Arc, Mutex},
    };
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        keyboard::{KeyCode, PhysicalKey},
        window::{Window, WindowId},
    };
    struct Logger(Mutex<File>);
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if let Ok(mut file) = self.0.lock() {
                let _ = writeln!(file, "{} {}", record.level(), record.args());
            }
        }
        fn flush(&self) {
            if let Ok(mut file) = self.0.lock() {
                let _ = file.flush();
            }
        }
    }
    #[derive(Default)]
    struct App {
        window: Option<Arc<Window>>,
        device: Option<Device>,
        texture: Option<Texture>,
    }
    impl App {
        fn draw(&mut self) -> Result<()> {
            let Some(device) = &mut self.device else {
                return Ok(());
            };
            if !device.begin([0.025, 0.05, 0.1])? {
                return Ok(());
            }
            let Some(texture) = &self.texture else {
                return Ok(());
            };
            let identity = [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ];
            device.transform(identity, identity, identity)?;
            device.state(
                DrawState {
                    lightmap: false,
                    cull: false,
                    mipmaps: false,
                    ..Default::default()
                },
                texture,
                None,
            )?;
            let quad = |left, top, right, bottom, z, color| {
                std::array::from_fn::<_, 4, _>(|i| Vertex {
                    pos: [
                        [left, top, z],
                        [right, top, z],
                        [right, bottom, z],
                        [left, bottom, z],
                    ][i],
                    uv: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][i],
                    color,
                    light: [0.0; 2],
                })
            };
            device.draw_transient(&quad(-0.8, 0.8, 0.5, -0.5, 0.7, 0xff80ffff))?;
            // Drawn later but further away: overlap must still show the first quad.
            device.draw_transient(&quad(-0.5, 0.5, 0.8, -0.8, 0.3, 0xffff8080))?;
            device.state(
                DrawState {
                    blend: Blend::Alpha,
                    lightmap: false,
                    cull: false,
                    depth_test: false,
                    depth_write: false,
                    alpha_test: false,
                    mipmaps: false,
                    ..Default::default()
                },
                texture,
                None,
            )?;
            device.draw_transient(&quad(-0.95, -0.55, 0.95, -0.9, 0.9, 0xafffc040))?;
            device.finish()?;
            Ok(())
        }
    }
    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let result = (|| -> Result<()> {
                let window = Arc::new(
                    event_loop.create_window(
                        Window::default_attributes()
                            .with_title("Horizon D3D9 HAL smoke - resize / Alt+Tab / Escape")
                            .with_inner_size(winit::dpi::LogicalSize::new(800, 600)),
                    )?,
                );
                let mut device = Device::new(window.clone(), true)?;
                let texture = device.texture(
                    2,
                    2,
                    1,
                    &[
                        255, 255, 255, 255, 40, 80, 255, 255, 255, 80, 40, 255, 255, 255, 255, 64,
                    ],
                )?;
                self.window = Some(window);
                self.device = Some(device);
                self.texture = Some(texture);
                Ok(())
            })();
            if let Err(e) = result {
                log::error!("D3D9 initialization: {e}");
                eprintln!("{e}");
                event_loop.exit();
            }
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::KeyboardInput { event, .. }
                    if event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
                {
                    event_loop.exit()
                }
                WindowEvent::Resized(size) => {
                    if let Some(device) = &mut self.device {
                        device.resize(size.width, size.height);
                    }
                }
                WindowEvent::RedrawRequested => {
                    if let Err(e) = self.draw() {
                        log::error!("D3D9 frame: {e}");
                        eprintln!("{e}");
                        event_loop.exit();
                    }
                }
                _ => {}
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(16),
            ));
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
    pub fn run() -> Result<()> {
        static LOGGER: std::sync::OnceLock<Logger> = std::sync::OnceLock::new();
        let path = std::env::var_os("HORIZON_D3D9_LOG").unwrap_or_else(|| "d3d9-smoke.log".into());
        let logger = LOGGER
            .get_or_init(|| Logger(Mutex::new(File::create(path).expect("create smoke log"))));
        let _ = log::set_logger(logger);
        log::set_max_level(log::LevelFilter::Info);
        EventLoop::new()?.run_app(&mut App::default())?;
        Ok(())
    }
}
#[cfg(windows)]
fn main() {
    if let Err(e) = windows_smoke::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This HAL smoke test requires Windows.");
}
