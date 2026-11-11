//! Process contract fixture, not a game and not an authentication bypass.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || args[0] != "--resources" {
        return Err("expected --resources PATH".into());
    }
    let home = std::env::var_os("HORIZON_HOME").ok_or("missing HORIZON_HOME")?;
    let cwd = std::env::current_dir()?;
    if std::fs::canonicalize(&home)? != std::fs::canonicalize(&cwd)? {
        return Err("cwd is not HORIZON_HOME".into());
    }
    if std::env::var("HORIZON_GRAPHICS_API")?.as_str() != "legacy-d3d9" {
        return Err("missing child-only legacy renderer selection".into());
    }
    let report = serde_json::json!({
        "mock": true, "resources": std::path::PathBuf::from(&args[1]),
        "home": std::path::PathBuf::from(&home), "cwd": cwd,
        "launcher": std::env::var_os("HORIZON_LAUNCHER_EXE").map(std::path::PathBuf::from),
        "inherited": std::env::var("HORIZON_SMOKE_INHERITED").ok(),
    });
    std::fs::write(
        cwd.join("mock-launch.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("mock-client: process contract recorded; no game was started");
    Ok(())
}
