//! Native controls entrypoint. The modern GPUI executable remains in main.rs.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![allow(dead_code)] // Shared CLI/MCP modules also serve the modern executable.

use horizon_launcher::backend;
#[cfg(test)]
use horizon_launcher::i18n;
#[path = "../cli.rs"]
mod cli;
#[path = "../cli_tools.rs"]
mod cli_tools;
#[path = "../console.rs"]
mod console;
#[path = "../game_tools.rs"]
mod game_tools;
#[path = "../mcp.rs"]
mod mcp;
#[path = "../perf.rs"]
#[allow(unused_imports)]
mod perf;
#[path = "../win7/mod.rs"]
mod win7;

fn main() {
    if let Err(error) = run() {
        #[cfg(windows)]
        if std::env::args_os().nth(1).is_none() {
            win7::window::error(&error);
        }
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    console::attach_for_cli().map_err(|e| e.to_string())?;
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--list-native-actions"))
    {
        println!(
            "{}",
            serde_json::to_string_pretty(&win7::catalog::actions()).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if win7::smoke::requested() {
        return win7::smoke::run();
    }
    // The legacy distribution is a portable ZIP. Do not invoke the modern installer's
    // shared HKCU identity or replace a registered modern launcher through quiet setup.
    if let Some(code) = horizon_launcher::relocation::run_worker() {
        std::process::exit(code);
    }
    let mut location_error = horizon_launcher::data_location::startup();
    backend::exe_path();
    if let Err(error) = backend::recover_legacy_installs() {
        location_error = Some(error);
    }
    if let Ok(error) = std::env::var("HORIZON_RELOCATION_ERROR") {
        location_error = Some(error);
    }
    if let Some(code) = horizon_launcher::update_handoff::run() {
        std::process::exit(code);
    }
    #[cfg(windows)]
    backend::remove_replaced_binaries();
    backend::cleanup_launcher_updates();
    horizon_launcher::proxy::announce(backend::Settings::load().proxy);
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new("--check-prepare-runtime"))
    {
        let arguments = clap::Command::new("--check-prepare-runtime")
            .arg(
                clap::Arg::new("compiler")
                    .long("compiler")
                    .action(clap::ArgAction::SetTrue),
            )
            .get_matches_from(std::env::args_os().skip(1));
        let home =
            backend::check_legacy_prepare_runtime(arguments.get_flag("compiler"), &|line| {
                eprintln!("{line}")
            })?;
        println!("{}", home.display());
        return Ok(());
    }
    if let Some(code) = cli::run() {
        std::process::exit(code);
    }
    #[cfg(windows)]
    return win7::window::run(location_error);
    #[cfg(not(windows))]
    {
        let _ = location_error;
        Err("The native window requires Windows; CLI commands are available on this host.".into())
    }
}
