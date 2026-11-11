//! Explicit launcher/process contract probe. This does not authenticate or launch a real game.
use std::path::PathBuf;

pub fn requested() -> bool {
    std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--smoke-client"))
}

pub fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(2).collect();
    if args.len() != 3 {
        return Err("--smoke-client PROGRAM EMPTY_HOME RESOURCES".into());
    }
    let home = PathBuf::from(&args[1]);
    if home.exists()
        && std::fs::read_dir(&home)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("The smoke test needs an empty home directory.".into());
    }
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let home = std::fs::canonicalize(home).map_err(|e| e.to_string())?;
    let command = horizon_launcher::backend::client_command_for(
        std::path::Path::new(&args[0]),
        &home,
        std::path::Path::new(&args[2]),
    );
    let (mut command, _runtime) = horizon_launcher::backend::prepare_command(command)?;
    let status = command.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(status.to_string())
    }
}
