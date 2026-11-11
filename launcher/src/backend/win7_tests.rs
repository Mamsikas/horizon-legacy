use super::*;
use std::ffi::OsStr;
use std::sync::atomic::{AtomicU64, Ordering};

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "horizon-win7-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn modern_windows_releases_cannot_substitute_for_signed_legacy_archives() {
    let releases = serde_json::json!([{
        "tag_name": "v0.9.0",
        "name": "Test release",
        "assets": [{
            "name": "horizon-windows-x64.zip",
            "browser_download_url": "https://example.invalid/horizon-windows-x64.zip",
            "size": 100
        }]
    }]);
    let (legacy_game, legacy_launcher) = profile_assets("windows", "x86_64", true).unwrap();
    let refused = game_releases_with(&releases, Some(legacy_game), None);
    assert_eq!(refused.len(), 1);
    assert!(refused[0].asset.is_none());
    let (modern_game, modern_launcher) = profile_assets("windows", "x86_64", false).unwrap();
    assert!(game_releases_with(&releases, Some(modern_game), None)[0]
        .asset
        .is_some());
    assert_ne!(modern_launcher, legacy_launcher);
    assert!(profile_assets("windows", "x86", true).is_none());
    assert!(profile_assets("windows", "aarch64", true).is_none());
}

#[test]
fn an_imported_program_cannot_inherit_its_bases_legacy_identity() {
    let dir = scratch("identity");
    let asset = "horizon-win7-x64.zip";
    fs::write(dir.join(CLIENT), b"verified legacy executable fixture").unwrap();
    assert!(!legacy_installation_matches(&dir, Some(asset)));
    record_legacy_install(&dir, asset, &"a".repeat(64)).unwrap();
    assert!(legacy_installation_matches(&dir, Some(asset)));
    assert!(!legacy_installation_matches(
        &dir,
        Some("horizon-windows-x64.zip")
    ));
    fs::write(dir.join(CLIENT), b"a modern imported executable fixture").unwrap();
    assert!(!legacy_installation_matches(&dir, Some(asset)));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_graphics_only_affects_the_intended_child_and_honors_explicit_selection() {
    let graphics = |cmd: &Command| {
        cmd.get_envs()
            .find(|(key, _)| *key == "HORIZON_GRAPHICS_API")
            .and_then(|(_, value)| value)
            .map(OsStr::to_owned)
    };
    let mut legacy = Command::new("fixture");
    apply_legacy_graphics(&mut legacy, true, false);
    assert_eq!(
        graphics(&legacy).as_deref(),
        Some(OsStr::new("legacy-d3d9"))
    );
    let mut modern = Command::new("fixture");
    apply_legacy_graphics(&mut modern, false, false);
    assert_eq!(graphics(&modern), None);
    let mut explicit = Command::new("fixture");
    apply_legacy_graphics(&mut explicit, true, true);
    assert_eq!(graphics(&explicit), None);
}

#[test]
fn failed_commit_restores_the_previous_installation_and_can_be_retried() {
    let root = scratch("rollback");
    let target = root.join("v0.9.0");
    fs::create_dir(&target).unwrap();
    fs::write(target.join(CLIENT), "old checked executable").unwrap();
    // A staged directory lost to an I/O failure must not destroy the previous version.
    assert!(replace_legacy_version(&root.join("missing"), &target).is_err());
    assert_eq!(
        fs::read_to_string(target.join(CLIENT)).unwrap(),
        "old checked executable"
    );
    let staged = root.join("staged");
    fs::create_dir(&staged).unwrap();
    fs::write(staged.join(CLIENT), "new checked executable").unwrap();
    replace_legacy_version(&staged, &target).unwrap();
    assert_eq!(
        fs::read_to_string(target.join(CLIENT)).unwrap(),
        "new checked executable"
    );
    assert!(!root.join("v0.9.0.win7-rollback").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn interrupted_commit_recovers_its_previous_directory_before_retrying() {
    let root = scratch("recover");
    let target = root.join("v0.9.0");
    let rollback = root.join("v0.9.0.win7-rollback");
    fs::create_dir(&rollback).unwrap();
    fs::write(rollback.join(CLIENT), "survived a stopped installer").unwrap();
    assert!(replace_legacy_version(&root.join("missing"), &target).is_err());
    assert_eq!(
        fs::read_to_string(target.join(CLIENT)).unwrap(),
        "survived a stopped installer"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn low_level_launch_contract_preserves_unicode_paths_without_shell_quoting() {
    let root = scratch("Кириллица с пробелами");
    let program = root.join("horizon-client.exe");
    let home = root.join("Мои миры");
    let resources = root.join("Мои ресурсы");
    let cmd = client_command_for(&program, &home, &resources);
    assert_eq!(cmd.get_program(), program.as_os_str());
    assert_eq!(cmd.get_current_dir(), Some(home.as_path()));
    assert_eq!(
        cmd.get_args().collect::<Vec<_>>(),
        [OsStr::new("--resources"), resources.as_os_str()]
    );
    assert_eq!(
        cmd.get_envs()
            .find(|(key, _)| *key == "HORIZON_HOME")
            .unwrap()
            .1,
        Some(home.as_os_str())
    );
    assert_eq!(
        cmd.get_envs()
            .find(|(key, _)| *key == "HORIZON_ACCOUNT_SESSIONS")
            .unwrap()
            .1,
        None
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_restores_interrupted_updates_without_listing_rollback_copies() {
    let root = scratch("startup-recovery");
    let rollback = root.join("v0.9.0.win7-rollback");
    fs::create_dir(&rollback).unwrap();
    fs::write(rollback.join(COMPLETE), "").unwrap();
    fs::write(rollback.join(CLIENT), "previous verified executable").unwrap();
    assert!(installed_in(&root).is_empty());
    assert_eq!(recover_legacy_installs_in(&root).unwrap(), ["v0.9.0"]);
    assert_eq!(installed_in(&root), ["v0.9.0"]);
    assert_eq!(
        fs::read_to_string(root.join("v0.9.0").join(CLIENT)).unwrap(),
        "previous verified executable"
    );
    assert!(recover_legacy_installs_in(&root).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_tag_modern_install_is_offered_its_legacy_replacement() {
    let root = scratch("same-tag-update");
    let dir = root.join("v0.9.0");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join(CLIENT), b"modern client fixture").unwrap();
    let releases = game_releases_with(
        &serde_json::json!([{
            "tag_name": "v0.9.0",
            "name": "Test release",
            "assets": [{
                "name": "horizon-win7-x64.zip",
                "browser_download_url": "https://example.invalid/horizon-win7-x64.zip",
                "size": 100
            }]
        }]),
        Some("horizon-win7-x64.zip"),
        None,
    );
    let installed = ["v0.9.0".to_owned()];
    let needs = |_: &str, release: &Release| {
        release.asset.is_some() && !legacy_installation_matches(&dir, Some("horizon-win7-x64.zip"))
    };
    assert!(horizon_update(&releases, &installed).is_none());
    let update = architecture_update_for(&releases, &installed, None, true, false, needs);
    assert_eq!(update.unwrap().tag, "v0.9.0");
    // Ordinary x64 modern launchers retain their no-choice behavior.
    assert!(architecture_update_for(&releases, &installed, None, false, false, needs).is_none());
    record_legacy_install(&dir, "horizon-win7-x64.zip", &"a".repeat(64)).unwrap();
    assert!(architecture_update_for(&releases, &installed, None, true, false, needs).is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn self_update_stages_the_executable_named_by_the_shipped_archive() {
    use sha2::Digest;
    let binary = if LEGACY_WINDOWS {
        "horizon-launcher-win7.exe"
    } else if cfg!(windows) {
        "horizon-launcher.exe"
    } else {
        "horizon-launcher"
    };
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file(binary, zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(b"new launcher executable fixture")
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    let size = bytes.len();
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let responder = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.write_all(&bytes).unwrap();
    });
    let root = scratch("launcher-stage");
    let staged = fill_stage(
        &root,
        "launcher@9.9.9",
        &format!("http://{address}/launcher.zip"),
        size as u64,
        &digest,
        &|_| {},
    )
    .unwrap();
    assert_eq!(staged.payload.file_name().unwrap(), binary);
    assert_eq!(
        fs::read(staged.payload).unwrap(),
        b"new launcher executable fixture"
    );
    responder.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recovery_does_not_interfere_with_an_installers_live_commit() {
    let root = scratch("competing-install");
    let rollback = root.join("v0.9.0.win7-rollback");
    fs::create_dir(&rollback).unwrap();
    fs::write(rollback.join(COMPLETE), "").unwrap();
    fs::write(rollback.join(CLIENT), b"previous executable").unwrap();
    let lease = legacy_install_lease(&root).unwrap();
    assert!(legacy_install_lease(&root).is_err());
    assert!(recover_legacy_installs_in(&root).is_err());
    assert!(rollback.join(CLIENT).is_file());
    assert!(!root.join("v0.9.0").exists());
    // Dropping an installer's file (also on process exit) makes crash recovery possible.
    drop(lease);
    assert_eq!(recover_legacy_installs_in(&root).unwrap(), ["v0.9.0"]);
    assert_eq!(
        fs::read(root.join("v0.9.0").join(CLIENT)).unwrap(),
        b"previous executable"
    );
    drop(legacy_install_lease(&root).unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_fork_requires_its_explicit_public_key_and_cannot_accept_an_arbitrary_checksum() {
    use base64::Engine;
    use horizon_release_sign::{sha256_hex, sums_file, SigningKey};
    let repository = Some("Mamsikas/horizon-legacy");
    assert!(release_keys_for(true, repository, None).is_err());
    assert!(release_keys_for(true, repository, Some("# no key\n")).is_err());
    assert!(release_keys_for(true, repository, Some("invalid")).is_err());
    assert!(release_keys_for(true, Some("owner/repo/another/path"), Some("invalid")).is_err());
    // These compile settings never change the modern launcher's trust policy.
    assert_eq!(
        release_keys_for(false, repository, Some("invalid")).unwrap(),
        horizon_release_sign::trusted_keys()
    );
    let (trusted, _) = SigningKey::generate().unwrap();
    let text = base64::engine::general_purpose::STANDARD.encode(trusted.public());
    let keys = release_keys_for(true, repository, Some(&text)).unwrap();
    let tag = "v9.9.9";
    let asset = "horizon-win7-x64.zip";
    let sums = sums_file(&[(asset.to_owned(), sha256_hex(b"fork release fixture"))]);
    assert!(
        horizon_release_sign::signed_digest(tag, asset, Some(sums.as_bytes()), None, &keys)
            .is_err()
    );
    let (other, _) = SigningKey::generate().unwrap();
    assert!(horizon_release_sign::signed_digest(
        tag,
        asset,
        Some(sums.as_bytes()),
        Some(&other.sign(tag, sums.as_bytes())),
        &keys
    )
    .is_err());
    assert_eq!(
        horizon_release_sign::signed_digest(
            tag,
            asset,
            Some(sums.as_bytes()),
            Some(&trusted.sign(tag, sums.as_bytes())),
            &keys
        )
        .unwrap(),
        sha256_hex(b"fork release fixture")
    );
}
