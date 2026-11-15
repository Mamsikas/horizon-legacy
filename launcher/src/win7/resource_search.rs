//! Shallow discovery of prepared resources; the caller supplies the core's readiness check.
//! No asset trees are traversed, copied or changed.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

const MANIFEST: &str = "horizon-resources.json";

fn resource_dir(root: &Path, version: &str) -> PathBuf {
    root.join(".local/horizon-client").join(version)
}

fn caches(root: &Path, version: &str, preferred: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let resources = resource_dir(&entry.path(), version);
            if !resources.join(MANIFEST).is_file() {
                continue;
            }
            let modified = fs::metadata(resources.join("horizon-prepare.json"))
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((entry.file_name() == preferred, modified, resources));
        }
    }
    // Prefer this preparer's cache, then the newest complete compatible output. The path
    // breaks timestamp ties consistently, independently of filesystem enumeration order.
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    found.into_iter().map(|(_, _, path)| path).collect()
}

pub(crate) fn find(
    chosen: &Path,
    version: &str,
    preferred_cache: &str,
    alternate_cache: Option<&Path>,
    validate: impl Fn(&Path) -> Result<(), String>,
) -> Result<PathBuf, Vec<(PathBuf, String)>> {
    let mut candidates = vec![
        chosen.to_path_buf(),
        resource_dir(chosen, version),
        chosen.join("resources"),
    ];
    candidates.extend(caches(
        &chosen.join("win7-prepare"),
        version,
        preferred_cache,
    ));
    if chosen
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("win7-prepare"))
    {
        candidates.extend(caches(chosen, version, preferred_cache));
    }
    if let Some(cache) = alternate_cache {
        candidates.extend(caches(cache, version, preferred_cache));
    }
    let mut seen = BTreeSet::new();
    let mut rejected = Vec::new();
    for resources in candidates {
        if !seen.insert(resources.clone()) || !resources.join(MANIFEST).is_file() {
            continue;
        }
        match validate(&resources) {
            Ok(()) => return Ok(resources),
            Err(error) => rejected.push((resources, error)),
        }
    }
    Err(rejected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // Validation belongs to the existing core. This test isolates directory discovery and
    // selection with original fixture bytes, including an unreadable/incompatible manifest.
    fn ready(path: &Path) -> Result<(), String> {
        match fs::read(path.join(MANIFEST)) {
            Ok(bytes) if bytes == b"ready" => Ok(()),
            _ => Err("incomplete or incompatible fixture".into()),
        }
    }

    fn cache(root: &Path, name: &str, state: &[u8], seconds: u64) -> PathBuf {
        let path = resource_dir(&root.join("win7-prepare").join(name), "26.3");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(MANIFEST), state).unwrap();
        let stamp = path.join("horizon-prepare.json");
        fs::write(&stamp, b"original fixture timestamp").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(stamp)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
        path
    }

    #[test]
    fn launcher_folder_finds_ready_caches_and_keeps_explicit_choices() {
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "horizon-resource-search-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
        )));
        let root = scratch.0.join("horizon-launcher");
        let empty = resource_dir(&root.join("win7-prepare/current"), "26.3");
        fs::create_dir_all(&empty).unwrap();
        let bad = cache(&root, "newest-unfinished", b"invalid", 300);
        let good = cache(&root, "older-complete", b"ready", 100);
        let choose = |path: &Path| find(path, "26.3", "current", None, ready).unwrap();
        assert_eq!(choose(&root), good);
        assert_eq!(choose(&root.join("win7-prepare")), good);
        assert_eq!(choose(&good), good);
        assert!(!empty.join("horizon-prepare.json").exists());
        // A newer valid output wins only when this preparer's cache is not ready.
        let newer = cache(&root, "newer-complete", b"ready", 200);
        assert_eq!(choose(&root), newer);
        let current = cache(&root, "current", b"ready", 50);
        assert_eq!(choose(&root), current);
        assert_eq!(choose(&good), good);
        // Selecting the launcher EXE folder can search the caller's configured cache.
        let exe_folder = scratch.0.join("launcher-exe");
        fs::create_dir_all(&exe_folder).unwrap();
        assert_eq!(
            find(
                &exe_folder,
                "26.3",
                "current",
                Some(&root.join("win7-prepare")),
                ready
            )
            .unwrap(),
            current
        );
        assert!(find(&exe_folder, "26.3", "current", None, ready)
            .unwrap_err()
            .is_empty());
        assert_eq!(fs::read(bad.join(MANIFEST)).unwrap(), b"invalid");
    }
}
