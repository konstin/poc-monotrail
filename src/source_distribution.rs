//! Build a wheel from a source distribution

use crate::utils::cache_dir;
use anyhow::{bail, Context, Result};
use fs_err as fs;
use install_wheel_rs::{WheelFilename, WheelInstallerError};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use tempfile::TempDir;

/// Takes a source distribution, checks whether we have already built a matching wheel, and if
/// not, builds a wheels from the source distribution by invoking `pip wheel --no-deps`
pub fn build_source_distribution_to_wheel_cached(
    name: &str,
    version: &str,
    sdist: &Path,
    compatible_tags: &[(String, String, String)],
) -> Result<PathBuf> {
    let target_dir = cache_dir()?.join("artifacts").join(name).join(version);

    let cache_hit = fs::read_dir(&target_dir).ok().and_then(|dir| {
        for entry in dir.flatten() {
            if !entry.path().to_string_lossy().ends_with(".whl") {
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
        let build_dir = TempDir::new()?;

        let wheel = build_to_wheel(sdist, build_dir.path(), compatible_tags)?;
        fs::create_dir_all(&target_dir)?;
        let wheel_in_cache = target_dir.join(wheel.file_name().unwrap_or(&OsString::new()));
        // rename only work on the same device :/
        fs::copy(wheel, &wheel_in_cache)?;
        Ok(wheel_in_cache)
    }
}

/// Builds a wheel from an source distribution or a repo checkout using `pip wheel --no-deps`
pub fn build_to_wheel(
    sdist_or_dir: &Path,
    // needs to be passed in or the tempdir will be deleted to early
    build_dir: &Path,
    compatible_tags: &[(String, String, String)],
) -> Result<PathBuf> {
    let output = Command::new("pip")
        .current_dir(build_dir)
        .args(&["wheel", "--no-deps"])
        .arg(sdist_or_dir)
        .output()
        .context("Failed to invoke pip")?;

    if !output.status.success() {
        return Err(WheelInstallerError::PythonSubcommandError(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Failed to run `pip wheel --no-deps {}`: {}\n---stdout:\n{}---stderr:\n{}",
                sdist_or_dir.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ))
        .into());
    } else {
        for path in fs::read_dir(build_dir)? {
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
