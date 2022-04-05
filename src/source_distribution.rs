use crate::wheel_tags::WheelFilename;
use crate::{package_index, WheelInstallerError};
use anyhow::{bail, Context, Result};
use fs_err as fs;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use tempfile::TempDir;

pub fn build_source_distribution_to_wheel_cached(
    name: &str,
    version: &str,
    sdist: &Path,
    compatible_tags: &[(String, String, String)],
) -> Result<PathBuf> {
    let target_dir = package_index::cache_dir()?
        .join("artifacts")
        .join(name)
        .join(version);

    let cache_hit = fs::read_dir(&target_dir).ok().and_then(|dir| {
        for entry in dir.flatten() {
            if !entry.path().ends_with(".whl") {
                continue;
            }
            if let Ok(true) = WheelFilename::from_str(entry.file_name().to_string_lossy().as_ref())
                .map(|filename| filename.is_compatible(compatible_tags))
            {
                return Some(entry.path());
            }
        }
        None
    });

    if let Some(cache_hit) = cache_hit {
        Ok(cache_hit)
    } else {
        let wheel = build_source_distribution_to_wheel(sdist, compatible_tags)?;
        fs::create_dir_all(&target_dir)?;
        let wheel_in_cache = target_dir.join(wheel.file_name().unwrap_or(&OsString::new()));
        fs::rename(wheel, &wheel_in_cache)?;
        Ok(wheel_in_cache)
    }
}

/// Builds a wheel using pip
pub fn build_source_distribution_to_wheel(
    sdist: &Path,
    compatible_tags: &[(String, String, String)],
) -> Result<PathBuf> {
    let build_dir = TempDir::new()?;

    let output = Command::new("pip")
        .current_dir(build_dir.path())
        .args(&["wheel", "--no-deps"])
        .arg(sdist)
        .output()
        .context("Failed to invoke pip")?;

    if !output.status.success() {
        return Err(WheelInstallerError::PythonSubcommandError(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Failed to run `pip wheel --no-deps {}`: {}\n---stdout:\n{}---stderr:\n{}",
                sdist.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ))
        .into());
    } else {
        for path in fs::read_dir(build_dir.path())? {
            let path = path?;
            let filename = path.file_name().to_string_lossy().to_string();
            if filename.ends_with(".whl") {
                if !WheelFilename::from_str(&filename)?.is_compatible(compatible_tags) {
                    bail!("pip wrote out an incompatible wheel (this is a bug)")
                }
                return Ok(path.path());
            }
        }
        bail!("pip didn't write out a wheel (dubious)")
    }
}
