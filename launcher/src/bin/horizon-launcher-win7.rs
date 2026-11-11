#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        use horizon_launcher::{backend, data_location, proxy, win7};
        let startup = data_location::startup();
        proxy::announce(backend::Settings::load().proxy);
        let error = startup.or_else(|| win7::flow::initialize().err());
        win7::window::run(error);
    }
    #[cfg(not(windows))]
    eprintln!(
        "{}",
        horizon_launcher::i18n::tr("The native Win7 launcher runs on Windows x64.")
    );
}
