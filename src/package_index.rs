use crate::WheelInstallerError;
use anyhow::{bail, Context, Result};
use fs_err as fs;
use std::collections::HashMap;
use std::path::PathBuf;
use std::{io, result};
use tracing::{debug, info};

fn search_wheel(name: &str, version: Option<&str>) -> Result<(PypiRelease, String)> {
    debug!("Getting Releases");
    let url = format!("https://pypi.org/pypi/{}/json", name);
    let pypi_project: PypiProject = ureq::get(&url)
        .set("User-Agent", "install-wheel-rs (konstin@mailbox.org)")
        .call()
        .context("Failed to contact pypi. Is your internet connection working?")?
        .into_json()
        .context("Invalid api response from pypi")?;
    if let Some(version) = version {
        let pypi_releases = pypi_project
            .releases
            .get(version)
            .with_context(|| format!("{} {} not found on pypi", name, version))?;
        let picked_wheel = pypi_releases
            .iter()
            .find(|release| {
                release.python_version == "py2.py3"
                    && release.packagetype == PackageType::BdistWheel
            })
            .with_context(|| {
                format!("Couldn't find compatible release for {} {}", name, version)
            })?;
        Ok((picked_wheel.clone(), version.to_string()))
    } else {
        let mut releases = pypi_project.releases.iter().collect::<Vec<_>>();
        // TODO: Actually parse versions
        releases.sort_by_key(|&(key, _)| key);
        releases.reverse();
        let mut picked_wheel = None;
        for (version, release) in releases {
            // TODO: Actually parse versions
            if let Some(picked_wheel_) = release.iter().find(|release| {
                release.python_version == "py2.py3"
                    && release.packagetype == PackageType::BdistWheel
            }) {
                picked_wheel = Some((picked_wheel_.to_owned(), version.to_string()));
                break;
            } else {
                eprint!(
                    "⚠️ No compatible package found for {} version {}",
                    name, version
                );
            }
        }
        if let Some((picked_wheel, version)) = picked_wheel {
            Ok((picked_wheel, version))
        } else {
            bail!("No matching version found for {}", name);
        }
    }
}

pub fn download_wheel(name: &str, version: Option<&str>) -> Result<PathBuf> {
    let (picked_wheel, version) = search_wheel(name, version)?;
    let target_dir = cache_dir()?.join("artifacts").join(name).join(version);
    let target_file = target_dir.join(&picked_wheel.filename);

    if target_file.is_file() {
        info!("Using cached download at {}", target_file.display());
        return Ok(target_file);
    }

    info!("Downloading wheel to {}", target_file.display());
    fs::create_dir_all(&target_dir).context("Couldn't create cache dir")?;
    // temp file so we don't clash with other processes running in parallel
    let mut temp_file = tempfile::NamedTempFile::new_in(&target_dir)
        .context("Couldn't create file for download")?;
    let request_for_file = ureq::get(&picked_wheel.url)
        .set("User-Agent", "install-wheel-rs (konstin@mailbox.org)")
        .call()
        .context("Failed to download file from pypi")?;
    io::copy(&mut request_for_file.into_reader(), &mut temp_file)
        .context("Failed to download wheel from pypi")?;
    temp_file
        .persist(&target_file)
        .context("Failed to moved wheel to target position")?;

    Ok(target_file)
}

/// https://warehouse.pypa.io/api-reference/json.html#get--pypi--project_name--json
#[derive(Deserialize, Clone, Debug)]
struct PypiProject {
    releases: HashMap<String, Vec<PypiRelease>>,
}

/// https://warehouse.pypa.io/api-reference/json.html#get--pypi--project_name--json
#[derive(Deserialize, Clone, Debug)]
#[allow(dead_code)]
struct PypiRelease {
    filename: String,
    packagetype: PackageType,
    python_version: String,
    size: usize,
    url: String,
}

/// https://github.com/pypa/warehouse/blob/4d4c7940063db51e8ee03de78afdff6d4e9140ae/warehouse/filters.py#L33-L41
#[derive(Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PackageType {
    BdistDmg,
    BdistDumb,
    BdistEgg,
    BdistMsi,
    BdistRpm,
    BdistWheel,
    BdistWininst,
    Sdist,
}

fn cache_dir() -> result::Result<PathBuf, WheelInstallerError> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| {
            WheelInstallerError::IOError(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a cache dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}
