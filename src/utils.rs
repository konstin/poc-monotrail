use anyhow::Context;
use fs_err as fs;
use fs_err::DirEntry;
use install_wheel_rs::WheelInstallerError;
use std::io;
use std::path::{Path, PathBuf};

/// Returns all subdirs in a directory
pub fn get_dir_content(dir: &Path) -> anyhow::Result<Vec<DirEntry>> {
    let read_dir = fs::read_dir(Path::new(&dir))
        .with_context(|| format!("Failed to load {} directory", env!("CARGO_PKG_NAME")))?;
    Ok(read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect())
}

/// `~/.cache/monotrail`
pub(crate) fn cache_dir() -> Result<PathBuf, WheelInstallerError> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| {
            WheelInstallerError::IOError(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a cache dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}

/// `~/.local/monotrail`
pub(crate) fn data_local_dir() -> Result<PathBuf, WheelInstallerError> {
    Ok(dirs::data_local_dir()
        .ok_or_else(|| {
            WheelInstallerError::IOError(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a data dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}
