//! Collect preparation inputs at build time, including implicit javac sourcepath dependencies.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const JAVA_ROOT: &str = "mods/horizon/src/test/java/dev/horizonlab/parity";

pub fn collect_sources(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::from([
        "research-lock.json".to_string(),
        "crates/horizon-prepare/src/exporters.rs".to_string(),
    ]);
    let mut pending = Vec::new();
    for entry in fs::read_dir(root.join("crates/horizon-prepare/src")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        for token in read(&path)?.split('"') {
            if token.ends_with(".java")
                && token.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
            {
                pending.push(format!("{JAVA_ROOT}/{token}"));
            }
        }
    }
    // Match references to our own compilation units only. JDK/Mojang classes are never copied.
    let mut units = BTreeMap::new();
    for entry in fs::read_dir(root.join(JAVA_ROOT)).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "java") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Invalid Java filename")?;
            units.insert(name.to_string(), format!("{JAVA_ROOT}/{name}.java"));
        }
    }
    while let Some(name) = pending.pop() {
        if !files.insert(name.clone()) {
            continue;
        }
        // Identifier matching also retains helpers named in reflective class strings. A few
        // extra references in comments are safe; silently dropping a helper is not.
        for identifier in read(&root.join(&name))?
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        {
            for name in [
                identifier,
                identifier.split('$').next().unwrap_or(identifier),
            ] {
                if let Some(dependency) = units.get(name) {
                    if !files.contains(dependency) {
                        pending.push(dependency.clone());
                    }
                }
            }
        }
    }
    Ok(files)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_transitive_and_reflective_helpers_without_unrelated_sources() {
        let root = std::env::temp_dir().join(format!("win7-source-bundle-{}", std::process::id()));
        let rust = root.join("crates/horizon-prepare/src");
        let java = root.join(JAVA_ROOT);
        fs::create_dir_all(&rust).unwrap();
        fs::create_dir_all(&java).unwrap();
        fs::write(rust.join("lib.rs"), "const SOURCE: &str = \"Entry.java\";").unwrap();
        for (name, text) in [
            ("Entry", "class Entry { Helper helper; }"),
            ("Helper", "class Helper { Leaf leaf; Entry cycle; }"),
            (
                "Leaf",
                "class Leaf { String reflected = \"dev.horizonlab.parity.Reflected$Nested\"; }",
            ),
            ("Reflected", "class Reflected {}"),
            ("Unrelated", "class Unrelated {}"),
        ] {
            fs::write(java.join(format!("{name}.java")), text).unwrap();
        }
        let bundled = collect_sources(&root).unwrap();
        let actual: BTreeSet<_> = bundled
            .iter()
            .filter(|p| p.ends_with(".java"))
            .cloned()
            .collect();
        assert_eq!(
            actual,
            ["Entry", "Helper", "Leaf", "Reflected"]
                .into_iter()
                .map(|name| format!("{JAVA_ROOT}/{name}.java"))
                .collect()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
