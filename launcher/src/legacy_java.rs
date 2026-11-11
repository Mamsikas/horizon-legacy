//! The legacy preparer's managed Java. Only vendor binaries are downloaded: no game files.
//! Liberica's Windows x64 package is a candidate for Win7 and includes its C runtime DLLs.
//! Import inspection is not a substitute for running the exporters on the target hardware.

use crate::backend;
use crate::i18n::{tr, trf};
use sha2::Digest;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

// Official BellSoft release assets and their GitHub asset SHA-256 digests, checked 2026-11-07.
// This October security update replaces 25+37, including its compromised CA roots.
const VERSION: &str = "25.0.5.0.3+5";
const RELEASE_VERSION: &str = "25.0.5.0.3";
const MARKER: &str = ".horizon-archive-sha256";

struct Package {
    url: &'static str,
    sha256: &'static str,
    size: u64,
    directory: &'static str,
    compiler: bool,
}

const JRE: Package = Package {
    url: "https://github.com/bell-sw/Liberica/releases/download/25.0.5.0.3%2B5/bellsoft-jre25.0.5.0.3%2B5-windows-amd64.zip",
    sha256: "6e1167a12a91fb5461f2cd304a91a5f4c4545dfb9e87a761a498c81e082414ae",
    size: 78_025_864,
    directory: "jre-25.0.5.0.3",
    compiler: false,
};

const JDK: Package = Package {
    url: "https://github.com/bell-sw/Liberica/releases/download/25.0.5.0.3%2B5/bellsoft-jdk25.0.5.0.3%2B5-windows-amd64.zip",
    sha256: "d7c90823ea2a24778d7499a82379b5e18c7ce3f4cc7a9d9c9af5b133abe148f0",
    size: 249_291_501,
    directory: "jdk-25.0.5.0.3",
    compiler: true,
};

pub(crate) fn prepare(major: i64, compiler: bool, log: &dyn Fn(String)) -> Result<PathBuf, String> {
    if std::env::consts::ARCH != "x86_64" || major != 25 {
        return Err(tr("This legacy launcher has a pinned preparation runtime only for Java 25 on Windows x64.").to_string());
    }
    backend::check_cancelled()?;
    let root = backend::data_dir().join("java").join("win7-x64");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(".install.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|e| {
        trf(
            "The managed Java installation is busy or cannot be locked: {error}",
            &[("error", &e.to_string())],
        )
    })?;
    let package = if compiler { &JDK } else { &JRE };
    let home = root.join(package.directory);
    let previous = root.join(format!(".previous-{}", package.directory));
    if previous.exists() && !home.exists() {
        fs::rename(&previous, &home).map_err(|e| e.to_string())?;
    }
    if fs::read_to_string(home.join(MARKER)).is_ok_and(|marker| marker == package.sha256)
        && layout(&home, package.compiler)
    {
        probe(&home, package.compiler)?;
        // A crash after publishing the new directory may leave the old copy behind.
        if previous.exists() {
            fs::remove_dir_all(&previous).map_err(|e| e.to_string())?;
        }
        return Ok(home);
    }
    if previous.exists() {
        return Err(trf(
            "Java recovery kept the previous installation at {path}; the current runtime could not be verified.",
            &[("path", &previous.display().to_string())],
        ));
    }
    let archive = root.join(format!("{}.zip", package.directory));
    // A successfully verified transfer survives a failed extraction or interrupted install.
    // A partial or damaged archive is never unpacked and is downloaded afresh.
    if verify(&archive, package.sha256).is_err() {
        backend::check_cancelled()?;
        let percent = std::cell::Cell::new(0);
        backend::download(
            package.url,
            package.size,
            package.sha256,
            &archive,
            &|fraction| {
                let now = ((fraction / 0.9 * 100.0) as u32).min(100) / 5 * 5;
                if now > percent.get() {
                    percent.set(now);
                    log(trf(
                        "Downloading Liberica Java {version}: {percent}%",
                        &[("version", VERSION), ("percent", &now.to_string())],
                    ));
                }
            },
        )?;
    }
    install_archive(&root, &archive, package, probe)?;
    let _ = fs::remove_file(archive);
    Ok(home)
}

fn verify(archive: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut digest = sha2::Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        backend::check_cancelled()?;
        let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    let actual = format!("{:x}", digest.finalize());
    horizon_release_sign::check("Liberica Java", expected, &actual)
}

fn layout(home: &Path, compiler: bool) -> bool {
    home.join("bin/java.exe").is_file()
        && (!compiler || home.join("bin/javac.exe").is_file())
        && crate::java::release_version(home).is_some_and(|(version, _)| version == RELEASE_VERSION)
}

fn probe(home: &Path, compiler: bool) -> Result<(), String> {
    for tool in ["java", "javac"]
        .into_iter()
        .take(if compiler { 2 } else { 1 })
    {
        backend::check_cancelled()?;
        let mut command = crate::process::command(home.join("bin").join(format!("{tool}.exe")));
        for variable in ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"] {
            command.env_remove(variable);
        }
        let output = command.arg("-version").output().map_err(|error| {
            trf(
                "Could not start the managed {tool}: {error}",
                &[("tool", tool), ("error", &error.to_string())],
            )
        })?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        if !output.status.success() || !text.contains(RELEASE_VERSION) {
            return Err(trf(
                "The managed {tool} failed its version check: {output}",
                &[("tool", tool), ("output", text.trim())],
            ));
        }
    }
    Ok(())
}

fn install_archive(
    root: &Path,
    archive: &Path,
    package: &Package,
    probe: impl FnOnce(&Path, bool) -> Result<(), String>,
) -> Result<(), String> {
    verify(archive, package.sha256)?;
    let stage = root.join(format!(".incoming-{}", std::process::id()));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        backend::unpack(archive, package.url, &stage)?;
        backend::check_cancelled()?;
        let incoming = stage.join(package.directory);
        if !layout(&incoming, package.compiler) {
            return Err(
                tr("The managed Java archive does not contain the expected runtime.").to_string(),
            );
        }
        // Test the exact staged executables; discovery must not select JAVA_HOME or PATH.
        probe(&incoming, package.compiler)?;
        fs::write(incoming.join(MARKER), package.sha256).map_err(|e| e.to_string())?;
        backend::check_cancelled()?;
        let home = root.join(package.directory);
        let previous = root.join(format!(".previous-{}", package.directory));
        if previous.exists() {
            return Err(tr("A previous Java installation is awaiting recovery; keep it and restart the launcher.").to_string());
        }
        let replacing = home.exists();
        if replacing {
            fs::rename(&home, &previous).map_err(|e| e.to_string())?;
        }
        if let Err(error) = fs::rename(&incoming, &home) {
            if replacing {
                fs::rename(&previous, &home).map_err(|rollback| {
                    trf(
                        "Java installation failed: {error}; restoring the previous installation also failed: {rollback}",
                        &[("error", &error.to_string()), ("rollback", &rollback.to_string())],
                    )
                })?;
            }
            return Err(error.to_string());
        }
        if replacing {
            let _ = fs::remove_dir_all(previous);
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_failed_runtime_probe_preserves_the_installed_runtime() {
        let root = std::env::temp_dir().join(format!("legacy-java-{}", std::process::id()));
        fs::create_dir_all(root.join(JRE.directory)).unwrap();
        fs::write(root.join(JRE.directory).join("old"), b"keep").unwrap();
        let archive = root.join("candidate.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for (name, data) in [
            ("bin/java.exe", "candidate"),
            ("release", "JAVA_VERSION=\"25.0.5.0.3\"\n"),
        ] {
            zip.start_file(
                format!("{}/{name}", JRE.directory),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let digest = format!("{:x}", sha2::Sha256::digest(fs::read(&archive).unwrap()));
        let package = Package {
            sha256: Box::leak(digest.into_boxed_str()),
            ..JRE
        };
        let result = install_archive(&root, &archive, &package, |_, _| {
            Err("runtime did not start".into())
        });
        assert_eq!(result.unwrap_err(), "runtime did not start");
        assert_eq!(
            fs::read(root.join(JRE.directory).join("old")).unwrap(),
            b"keep"
        );
        assert!(!root.join(JRE.directory).join(MARKER).exists());
        // A corrupt cached archive is rejected before the supplied probe can run.
        fs::write(&archive, b"damaged").unwrap();
        assert!(install_archive(&root, &archive, &package, |_, _| panic!(
            "unchecked archive executed"
        ))
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
