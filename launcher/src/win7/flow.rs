//! Setup and offline launch using the existing download, source and preparation core.
use crate::{
    backend,
    i18n::{tr, trf},
    sources,
};
use horizon_platform::prepare;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod support {
    include!(concat!(env!("OUT_DIR"), "/win7_support.rs"));
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub nickname: String,
    pub minecraft: PathBuf,
    /// Empty chooses a bundled client, then an installed official release.
    pub client: PathBuf,
    pub installed: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            nickname: "Player".into(),
            minecraft: backend::default_minecraft_dir().unwrap_or_default(),
            client: PathBuf::new(),
            installed: String::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        fs::read(backend::data_dir().join("win7-launcher.json"))
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(backend::data_dir()).map_err(|e| e.to_string())?;
        let path = backend::data_dir().join("win7-launcher.json");
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(
            &temp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(temp, path).map_err(|e| e.to_string())
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(3..=16).contains(&self.nickname.len())
            || !self
                .nickname
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(tr("Use a nickname of 3–16 letters, numbers or underscores.").into());
        }
        if !self.client.as_os_str().is_empty() && !self.client.is_file() {
            return Err(tr(
                "The selected Horizon client was not found. Choose its executable again.",
            )
            .into());
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Event {
    Status(String),
    Log(String),
    Progress(f32),
    Playing,
    Finished(Result<(), String>),
}

pub fn log_path() -> PathBuf {
    backend::data_dir().join("win7-launcher.log")
}
pub fn cancel_path() -> PathBuf {
    backend::data_dir().join(format!(".win7-cancel-{}", std::process::id()))
}

/// Called before the window starts any threads. These settings affect only preparation.
pub fn initialize() -> Result<(), String> {
    fs::create_dir_all(backend::data_dir()).map_err(|e| e.to_string())?;
    let cancel = cancel_path();
    let _ = fs::remove_file(&cancel);
    std::env::set_var("HORIZON_LAUNCHER_CANCEL_FILE", &cancel);
    std::env::set_var("HORIZON_PREPARE_CANCEL_FILE", &cancel);
    for (key, value) in [
        ("HORIZON_PREPARE_JOBS", "1"),
        ("HORIZON_PREPARE_MAX_HEAP_MIB", "512"),
    ] {
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

pub fn run(mut settings: Settings, emit: &dyn Fn(Event)) -> Result<(), String> {
    settings.validate()?;
    settings.save()?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(backend::data_dir().join(".win7-launch.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|_| {
        tr("Horizon is already starting or running in another launcher.").to_string()
    })?;
    emit(Event::Log(trf(
        "Launcher {version}; {os}/{arch}",
        &[
            ("version", backend::VERSION),
            ("os", std::env::consts::OS),
            ("arch", std::env::consts::ARCH),
        ],
    )));
    let resources = resources(&settings.minecraft, emit).map_err(|error| {
        trf(
            "Resource preparation failed before the game was started.\n{error}",
            &[("error", &error)],
        )
    })?;
    backend::check_cancelled()?;
    emit(Event::Status(tr("Getting Horizon ready…").into()));
    let (program, managed) = client(&mut settings, emit)?;
    settings.save()?;
    backend::check_cancelled()?;
    let home = backend::game_home();
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let game_log = backend::data_dir().join("win7-game.log");
    let output = fs::File::create(&game_log).map_err(|e| e.to_string())?;
    let mut cmd = offline_command(&program, &resources, &home, &settings.nickname);
    // Only installed releases need the core's stable update/runtime slot. Local clients
    // run in place: keep their chosen filename and avoid copying an entire user folder.
    #[cfg(windows)]
    if managed {
        crate::windows_runtime::mark(&mut cmd, program.parent().unwrap());
    }
    #[cfg(not(windows))]
    let _ = (managed, &mut cmd);
    let (mut cmd, _runtime) = backend::prepare_command(cmd)?;
    cmd.stdin(Stdio::null())
        .stderr(output.try_clone().map_err(|e| e.to_string())?)
        .stdout(output);
    emit(Event::Log(trf(
        "Starting {path}",
        &[("path", &program.display().to_string())],
    )));
    let mut child = cmd.spawn().map_err(|e| {
        trf(
            "Could not start Horizon: {error}",
            &[("error", &e.to_string())],
        )
    })?;
    emit(Event::Playing);
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(trf(
            "Horizon exited with {status}. Open the game log for details.",
            &[("status", &status.to_string())],
        ))
    }
}

/// No Microsoft token, fake session or global graphics change. The stock client already
/// supports an offline player name; a future DX9 build uses this same launch contract.
pub fn offline_command(program: &Path, resources: &Path, home: &Path, nickname: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(home)
        .env("HORIZON_HOME", home)
        .env("HORIZON_PLAYER_NAME", nickname)
        .args(["--resources"])
        .arg(resources)
        .arg("--game-dir")
        .arg(home)
        .arg("--home")
        .arg(home)
        .arg("--name")
        .arg(nickname);
    for key in backend::LAUNCHER_RENDER_ENV.into_iter().chain([
        "HORIZON_SESSION_FILE",
        "HORIZON_ACCOUNT_SESSIONS",
        "HORIZON_LAUNCHER_EXE",
        "HORIZON_PREPARE_CANCEL_FILE",
        "HORIZON_LAUNCHER_CANCEL_FILE",
    ]) {
        cmd.env_remove(key);
    }
    crate::process::hide_console(&mut cmd);
    cmd
}

fn client(settings: &mut Settings, emit: &dyn Fn(Event)) -> Result<(PathBuf, bool), String> {
    if !settings.client.as_os_str().is_empty() {
        return Ok((settings.client.clone(), false));
    }
    if let Some(bundled) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("horizon-client.exe")))
        .filter(|p| p.is_file())
    {
        return Ok((bundled, false));
    }
    let installed = backend::installed();
    if !installed.contains(&settings.installed) {
        settings.installed = installed.first().cloned().unwrap_or_default();
    }
    if settings.installed.is_empty() {
        emit(Event::Status(tr("Downloading Horizon…").into()));
        let releases = backend::releases()?;
        let release = backend::latest(&releases).ok_or_else(|| {
            tr("No Horizon download is available. Choose a local client executable.").to_string()
        })?;
        backend::install(release, |p| emit(Event::Progress(p)))?;
        settings.installed = release.tag.clone();
    }
    let dir = backend::version_dir(&settings.installed);
    // Do not silently feed a newer, incompatible game release this preparer's output.
    let lock: Value = serde_json::from_slice(
        &fs::read(dir.join("research-lock.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if lock["minecraft"]["version"] != horizon_platform::version::VANILLA_VERSION {
        return Err(tr("This client needs a different Minecraft version. Choose a Horizon client for Minecraft 26.3.").into());
    }
    let lib = fs::read_to_string(dir.join("crates/horizon-prepare/src/lib.rs"))
        .map_err(|e| e.to_string())?;
    let expected = format!(
        "pub const OUTPUT_VERSION: u32 = {};",
        prepare::OUTPUT_VERSION
    );
    if !lib.lines().any(|line| line == expected) {
        return Err(tr("This Horizon version needs a newer resource preparer. Update the Win7 launcher or choose a compatible client.").into());
    }
    let exe = dir.join(if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    });
    if !exe.is_file() {
        return Err(tr("The installed Horizon client executable is missing.").into());
    }
    Ok((exe, true))
}

fn support_root() -> Result<PathBuf, String> {
    let root = backend::build_dir()
        .join("win7-prepare")
        .join(prepare::SOURCES_SHA256);
    for &(name, bytes) in support::FILES {
        let file = root.join(name);
        if fs::read(&file).is_ok_and(|old| old == bytes) {
            continue;
        }
        fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(file, bytes).map_err(|e| e.to_string())?;
    }
    Ok(root)
}

fn resources(chosen: &Path, emit: &dyn Fn(Event)) -> Result<PathBuf, String> {
    let root = support_root()?;
    emit(Event::Log(trf(
        "Preparation folder: {path}",
        &[("path", &root.display().to_string())],
    )));
    let resources = prepare::resources_dir(&root, horizon_platform::version::VANILLA_VERSION, None);
    emit(Event::Status(tr("Checking game files…").into()));
    let log = |s: String| emit(Event::Log(s));
    if prepare::check(&root, &resources).is_ok_and(|missing| missing.is_empty()) {
        return Ok(resources);
    }
    let pins: Value = serde_json::from_slice(
        &fs::read(root.join("research-lock.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let source_pins = sources::Pins::read(&pins)
        .ok_or_else(|| tr("The bundled Minecraft file pins are invalid.").to_string())?;
    let known = sources::Folders::current()
        .map(|f| sources::known_folders(sources::Os::current(), &f))
        .unwrap_or_default();
    let minecraft = match sources::resolve(
        chosen,
        &source_pins,
        &known,
        &root.join("minecraft-source"),
        &backend::get_bytes,
        &log,
    )? {
        sources::Resolved::Folder(dir) => dir,
        sources::Resolved::NotFound(missing) => {
            for (path, reason) in missing {
                log(format!("{}: {reason}", path.display()));
            }
            return Err(tr("Minecraft 26.3 files were not found or did not match the official checksums. Install that version with your Minecraft launcher, then choose its game folder here.").into());
        }
    };
    backend::check_cancelled()?;
    let manifest_path = minecraft
        .join("versions")
        .join(&source_pins.version)
        .join(format!("{}.json", source_pins.version));
    let manifest: Value =
        serde_json::from_slice(&fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let server = root.join("server.jar");
    let expected = pins["minecraft"]["server_sha1"]
        .as_str()
        .ok_or("Missing server hash")?;
    if sources::sha1_file(&server).as_deref() != Some(expected) {
        emit(Event::Status(
            tr("Downloading official preparation files…").into(),
        ));
        download_server(&manifest["downloads"]["server"], expected, &server, emit)?;
    }
    emit(Event::Status(
        tr("Setting up the preparation runtime…").into(),
    ));
    let java = crate::legacy_java::prepare(25, true, &log)?;
    // This process has one preparation worker. Pin discovery to the verified managed JVM.
    std::env::set_var("JAVA_HOME", &java);
    log(trf(
        "Preparation compiler: {path}",
        &[("path", &java.join("bin/javac.exe").display().to_string())],
    ));
    log(trf(
        "Preparation Java: {path}",
        &[("path", &java.join("bin/java.exe").display().to_string())],
    ));
    log(trf(
        "Preparation heap limit: {mib} MiB",
        &[(
            "mib",
            &std::env::var("HORIZON_PREPARE_MAX_HEAP_MIB").unwrap_or_default(),
        )],
    ));
    emit(Event::Status(
        tr("Preparing Minecraft 26.3 — first setup may take a while…").into(),
    ));
    prepare::prepare_with_progress(
        &prepare::Options {
            root: root.clone(),
            import: true,
            minecraft_dir: Some(minecraft),
            server_jar: Some(server),
            java_home: Some(java),
            inputs: Default::default(),
            output: None,
        },
        &|s| log(s.into()),
        &|p| emit(Event::Progress(p)),
    )?;
    backend::check_cancelled()?;
    let missing = prepare::check(&root, &resources)?;
    if !missing.is_empty() {
        return Err(missing.join("\n"));
    }
    Ok(resources)
}

fn download_server(
    file: &Value,
    expected: &str,
    path: &Path,
    emit: &dyn Fn(Event),
) -> Result<(), String> {
    if file["sha1"].as_str() != Some(expected) {
        return Err(
            tr("The official server file does not match the pinned Minecraft version.").into(),
        );
    }
    let url = file["url"]
        .as_str()
        .filter(|s| s.starts_with("https://"))
        .ok_or("Missing HTTPS download URL")?;
    let total = file["size"].as_u64().ok_or("Missing download size")?;
    let temp = path.with_extension("part");
    let result = (|| {
        let mut reader = backend::get(url)?.into_reader();
        let mut output = fs::File::create(&temp).map_err(|e| e.to_string())?;
        let mut hash = Sha1::new();
        let mut buffer = [0; 65536];
        let mut count = 0u64;
        loop {
            backend::check_cancelled()?;
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > total {
                return Err(tr("The download exceeded its expected size.").into());
            }
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            hash.update(&buffer[..n]);
            emit(Event::Progress(count as f32 / total.max(1) as f32));
        }
        if count != total || format!("{:x}", hash.finalize()) != expected {
            return Err(
                tr("The download failed its checksum or size check. Retry the setup.").into(),
            );
        }
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_exporters_include_every_compiler_input() {
        for name in prepare::exporters::source_files() {
            let path = format!("mods/horizon/src/test/java/dev/horizonlab/parity/{name}");
            assert!(
                support::FILES
                    .iter()
                    .any(|(file, bytes)| *file == path && !bytes.is_empty()),
                "missing bundled Java source: {name}"
            );
        }
    }

    #[test]
    fn offline_launch_keeps_paths_and_name_separate_and_removes_sessions() {
        let cmd = offline_command(
            Path::new("C:/My Game/custom-client.exe"),
            Path::new("C:/Данные/resources"),
            Path::new("C:/My Game/home"),
            "Alex_7",
        );
        assert_eq!(cmd.get_program(), "C:/My Game/custom-client.exe");
        let args: Vec<_> = cmd.get_args().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "--resources",
                "C:/Данные/resources",
                "--game-dir",
                "C:/My Game/home",
                "--home",
                "C:/My Game/home",
                "--name",
                "Alex_7"
            ]
        );
        let env: std::collections::BTreeMap<_, _> = cmd.get_envs().collect();
        assert_eq!(
            env[std::ffi::OsStr::new("HORIZON_PLAYER_NAME")],
            Some(std::ffi::OsStr::new("Alex_7"))
        );
        assert_eq!(env[std::ffi::OsStr::new("HORIZON_SESSION_FILE")], None);
        assert!(!env.contains_key(std::ffi::OsStr::new("HORIZON_GRAPHICS_API")));
        assert_eq!(cmd.get_current_dir(), Some(Path::new("C:/My Game/home")));
    }
}
