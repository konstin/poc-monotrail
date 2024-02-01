//! Build a wheel from a source distribution

use crate::utils::cache_dir;
use anyhow::{bail, Context, Result};
use fs_err as fs;
use install_wheel_rs::{CompatibleTags, Error, WheelFilename};
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
    compatible_tags: &CompatibleTags,
) -> Result<PathBuf> {
    let target_dir = cache_dir()?.join("artifacts").join(name).join(version);

    if let Ok(target_dir) = fs::read_dir(&target_dir) {
        for entry in target_dir.flatten() {
            if !entry.path().to_string_lossy().ends_with(".whl") {
                continue;
            }
            if let Ok(true) = WheelFilename::from_str(entry.file_name().to_string_lossy().as_ref())
                .map(|filename| filename.compatibility(compatible_tags).is_ok())
            {
                return Ok(entry.path());
            }
        }
    };

    let build_dir = TempDir::new()?;
    let wheel = build_to_wheel(sdist, build_dir.path(), compatible_tags)?;
    fs::create_dir_all(&target_dir)?;
    let wheel_in_cache = target_dir.join(wheel.file_name().unwrap_or(&OsString::new()));
    // rename only work on the same device :/
    fs::copy(wheel, &wheel_in_cache)?;
    Ok(wheel_in_cache)
}

/// Builds a wheel from an source distribution or a repo checkout using `pip wheel --no-deps`
pub fn build_to_wheel(
    sdist_or_dir: &Path,
    // needs to be passed in or the tempdir will be deleted to early
    build_dir: &Path,
    compatible_tags: &CompatibleTags,
) -> Result<PathBuf> {
    let output = Command::new("pip")
        .current_dir(build_dir)
        .args(["wheel", "--no-deps"])
        .arg(sdist_or_dir)
        .output()
        .context("Failed to invoke pip")?;

    if !output.status.success() {
        return Err(Error::PythonSubcommand(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Failed to run `pip wheel --no-deps {}`: {}\n---stdout:\n{}---stderr:\n{}\n---",
                sdist_or_dir.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
        .into());
    }

    for path in fs::read_dir(build_dir)? {
        let path = path?;
        let filename = path.file_name().to_string_lossy().to_string();
        if filename.ends_with(".whl") {
            if WheelFilename::from_str(&filename)?
                .compatibility(compatible_tags)
                .is_err()
            {
                bail!(
                    "pip wrote out an incompatible wheel. \
                    This is a bug, either in monotrail or in pip"
                )
            }
            return Ok(path.path());
        }
    }
    bail!("pip didn't write out a wheel (dubious)")
}
