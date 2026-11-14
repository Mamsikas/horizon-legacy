//! Embeds Horizon's icon (`packaging/icons/horizon.ico`) in the Windows executables, as icon
//! resource 1: Explorer and the task bar show it, and the window takes it as its icon.
#[path = "build_support.rs"]
mod build_support;

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
// Include the transitive same-package helpers javac loads from its sourcepath.
fn embed_win7_support() {
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("..");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/horizon-prepare/src").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join(build_support::JAVA_ROOT).display()
    );
    let files =
        build_support::collect_sources(&root).expect("collecting preparation source dependencies");
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
