use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub(super) fn write_checked(
    mut reader: impl Read,
    size: u64,
    name: &str,
    sha256: &str,
    to: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    let mut file = fs::File::create(to).map_err(|e| e.to_string())?;
    let mut hash = horizon_release_sign::Sha256::default();
    let (mut done, mut buf) = (0u64, vec![0u8; 1 << 16]);
    loop {
        check_cancelled()?;
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress((done as f32 / size.max(1) as f32).min(1.0) * 0.9);
    }
    horizon_release_sign::check(name, sha256, &hash.hex())
}

/// A cooperative download boundary used by the native launcher worker.
pub(crate) fn check_cancelled() -> Result<(), String> {
    if cfg!(feature = "win7")
        && std::env::var_os("HORIZON_LAUNCHER_CANCEL_FILE").is_some_and(|p| Path::new(&p).exists())
    {
        return Err(crate::i18n::tr("Operation cancelled.").into());
    }
    Ok(())
}
