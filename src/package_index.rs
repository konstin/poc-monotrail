//! Basic downloading from pypi

use crate::spec::DistributionType;
use crate::wheel_tags::WheelFilename;
use crate::WheelInstallerError;
use anyhow::{bail, Context, Result};
use fs_err as fs;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{io, result};
use tracing::debug;

/// https://warehouse.pypa.io/api-reference/json.html#get--pypi--project_name--json
#[derive(Deserialize, Clone, Debug)]
struct PypiProject {
    releases: HashMap<String, Vec<PypiRelease>>,
}

/// https://warehouse.pypa.io/api-reference/json.html#get--pypi--project_name--json
#[derive(Deserialize, Clone, Debug)]
#[allow(dead_code)]
pub struct PypiRelease {
    pub filename: String,
    pub packagetype: PackageType,
    pub python_version: String,
    pub size: usize,
    pub url: String,
}

/// https://github.com/pypa/warehouse/blob/4d4c7940063db51e8ee03de78afdff6d4e9140ae/warehouse/filters.py#L33-L41
#[derive(Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    BdistDmg,
    BdistDumb,
    BdistEgg,
    BdistMsi,
    BdistRpm,
    BdistWheel,
    BdistWininst,
    Sdist,
}

fn matching_package_for_version(
    _name: &str,
    compatible_tags: &[(String, String, String)],
    version: &str,
    pypi_releases: &[PypiRelease],
) -> Result<Option<(PypiRelease, DistributionType, String)>> {
    let wheel_releases = pypi_releases
        .iter()
        .filter(|release| release.packagetype == PackageType::BdistWheel)
        .map(|release| Ok((WheelFilename::from_str(&release.filename)?, release)))
        .collect::<Result<Vec<(WheelFilename, &PypiRelease)>, WheelInstallerError>>()?;
    if let Some((_, picked_wheel)) = wheel_releases
        .iter()
        .find(|(filename, _)| filename.is_compatible(compatible_tags))
    {
        return Ok(Some((
            (*picked_wheel).clone(),
            DistributionType::Wheel,
            version.to_string(),
        )));
    }

    if let Some(sdist_release) = pypi_releases
        .iter()
        .find(|release| release.packagetype == PackageType::Sdist)
    {
        Ok(Some((
            sdist_release.clone(),
            DistributionType::SourceDistribution,
            version.to_string(),
        )))
    } else {
        Ok(None)
    }
}

/// Finds a matching wheel from pages like https://pypi.org/pypi/tqdm/json
///
/// https://warehouse.pypa.io/api-reference/json.html
pub fn search_release(
    name: &str,
    version: Option<String>,
    compatible_tags: &[(String, String, String)],
) -> Result<(PypiRelease, DistributionType, String)> {
    debug!("Getting Releases");
    let url = format!("https://pypi.org/pypi/{}/json", name);
    let pypi_project: PypiProject = ureq::get(&url)
        .set("User-Agent", "virtual-sprawl (konstin@mailbox.org)")
        .call()
        .context("Failed to contact pypi. Is your internet connection working?")?
        .into_json()
        .context("Invalid api response from pypi")?;
    if let Some(version) = version {
        let pypi_releases = pypi_project
            .releases
            .get(&version)
            .with_context(|| format!("{} {} not found on pypi", name, version))?;

        matching_package_for_version(name, compatible_tags, &version, pypi_releases)?
            .with_context(|| format!("Couldn't find compatible release for {} {}", name, version))
    } else {
        let mut releases = pypi_project.releases.iter().collect::<Vec<_>>();
        // TODO: Actually parse versions
        releases.sort_by_key(|&(key, _)| key);
        releases.reverse();
        for (version, release) in releases {
            if let Some(matching_package) =
                matching_package_for_version(name, compatible_tags, version, release)?
            {
                return Ok(matching_package);
            }
        }
        bail!("No matching version found for {}", name);
    }
}

/// Just wraps ureq
pub(crate) fn download_distribution(
    url: &str,
    target_dir: &Path,
    target_file: &Path,
) -> Result<()> {
    debug!("Downloading wheel to {}", target_file.display());
    fs::create_dir_all(&target_dir).context("Couldn't create cache dir")?;
    // temp file so we don't clash with other processes running in parallel
    let mut temp_file = tempfile::NamedTempFile::new_in(&target_dir)
        .context("Couldn't create file for download")?;
    let request_for_file = ureq::get(url)
        .set("User-Agent", "virtual-sprawl (konstin@mailbox.org)")
        .call()
        .context("Error during pypi request")?;
    io::copy(&mut request_for_file.into_reader(), &mut temp_file)
        .context("Failed to download wheel from pypi")?;
    temp_file
        .persist(&target_file)
        .context("Failed to moved wheel to target position")?;
    Ok(())
}

/// `~/.cache/virtual-sprawl`
pub(crate) fn cache_dir() -> result::Result<PathBuf, WheelInstallerError> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| {
            WheelInstallerError::IOError(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a cache dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}
