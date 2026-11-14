//! Embeds Horizon's icon (`packaging/icons/horizon.ico`) in the Windows executables, as icon
//! resource 1: Explorer and the task bar show it, and the window takes it as its icon.
fn main() {
    if std::env::var_os("CARGO_FEATURE_WIN7").is_some() {
        embed_win7_support();
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Resolve imports before main from System32, never a nearby injector DLL (#13108).
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ico = format!("{dir}/../packaging/icons/horizon.ico");
    println!("cargo:rerun-if-changed={ico}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(&ico);
        if let Err(e) = resource.compile() {
            panic!("embedding {ico}: {e}");
        }
    }
}

// Ship only Horizon's original exporters and their pins, never Mojang's files.
// Source selection matches tools/release_train.py::exporters.
fn embed_win7_support() {
    use std::{collections::BTreeSet, fs, path::PathBuf};
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let mut files = BTreeSet::from([
        "research-lock.json".to_string(),
        "crates/horizon-prepare/src/exporters.rs".to_string(),
    ]);
    for source in fs::read_dir(root.join("crates/horizon-prepare/src")).unwrap() {
        let source = source.unwrap().path();
        if source.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", source.display());
        for token in fs::read_to_string(&source).unwrap().split('"') {
            if token.ends_with(".java")
                && token.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
            {
                files.insert(format!(
                    "mods/horizon/src/test/java/dev/horizonlab/parity/{token}"
                ));
            }
        }
    }
    let mut code = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    for name in files {
        let path = root
            .join(&name)
            .canonicalize()
            .expect("exporter source missing");
        println!("cargo:rerun-if-changed={}", path.display());
        code.push_str(&format!(
            "({name:?}, include_bytes!({:?})),\n",
            path.to_str().unwrap()
        ));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("win7_support.rs"),
        code,
    )
    .unwrap();
}
