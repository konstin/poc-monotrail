//! Filter and install python packages with install-wheel-rs

use crate::cli::download_distribution_cached;
use crate::monotrail::filter_installed_monotrail;
use crate::source_distribution::build_source_distribution_to_wheel_cached;
use crate::spec::{DistributionType, FileOrUrl, RequestedSpec};
use anyhow::{bail, Context};
use fs_err as fs;
use fs_err::{DirEntry, File};
use git2::{Direction, Repository};
use indicatif::{ProgressBar, ProgressStyle};
use install_wheel_rs::{install_wheel, parse_key_value_file, InstallLocation, LockedDir};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::Serialize;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tracing::{debug, info, trace, warn};

/// what we communicate back to python
#[cfg(not(feature = "python_bindings"))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InstalledPackage {
    pub name: String,
    pub python_version: String,
    pub unique_version: String,
    /// The compatibility tag like "py3-none-any" or
    /// "cp38-cp38-manylinux_2_12_x86_64.manylinux2010_x86_64"
    pub tag: String,
}

/// TODO: write a pyo3 bug report to parse through cfg attr
#[cfg(feature = "python_bindings")]
#[pyo3::pyclass]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InstalledPackage {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub python_version: String,
    #[pyo3(get)]
    pub unique_version: String,
    #[pyo3(get)]
    pub tag: String,
}

#[cfg_attr(feature = "python_bindings", pyo3::pymethods)]
impl InstalledPackage {
    /// PathBuf for pyo3
    pub fn monotrail_location(&self, sprawl_root: PathBuf) -> PathBuf {
        sprawl_root
            .join(&self.name)
            .join(&self.unique_version)
            .join(&self.tag)
    }

    pub fn monotrail_site_packages(
        &self,
        sprawl_root: PathBuf,
        // keep it around, in case we need to switch back because someone's depending on pythonx.y
        // folders for location stuff
        _python_version: (u8, u8),
    ) -> PathBuf {
        self.monotrail_location(sprawl_root)
            .join("lib")
            .join("python")
            .join("site-packages")
    }
}

/// Reads the installed packages through .dist-info/WHEEL files, returns the set that is installed
/// and the one that still needs to be installed
pub fn filter_installed_venv(
    specs: &[RequestedSpec],
    venv_base: &Path,
    python_version: (u8, u8),
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    let site_packages = venv_base
        .join("lib")
        .join(format!("python{}.{}", python_version.0, python_version.1))
        .join("site-packages");
    let entries: Vec<DirEntry> = match fs::read_dir(site_packages) {
        Ok(entries) => entries.collect::<io::Result<Vec<DirEntry>>>()?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let venv_packages: Vec<InstalledPackage> = entries
        .iter()
        .filter_map(|entry| {
            let filename = entry.file_name().to_string_lossy().to_string();
            let (name, version) = filename.strip_suffix(".dist-info")?.split_once('-')?;
            let name = name.to_lowercase().replace('-', "_");
            Some((entry, name, version.to_string()))
        })
        .map(|(entry, name, version)| {
            let wheel_data =
                parse_key_value_file(&mut File::open(entry.path().join("WHEEL"))?, "WHEEL")?;
            let tag = wheel_data
                .get("Tag")
                .map(|tags| tags.join("."))
                .unwrap_or_default();

            Ok(InstalledPackage {
                name,
                python_version: version.clone(),
                unique_version: version,
                tag,
            })
        })
        .collect::<anyhow::Result<_>>()?;

    let mut installed = Vec::new();
    let mut not_installed = Vec::new();
    for spec in specs {
        let matching_package = venv_packages.iter().find(|package| {
            if let Some(spec_version) = &spec.python_version {
                // TODO: use PEP440
                package.name == spec.name && &package.python_version == spec_version
            } else {
                package.name == spec.name
            }
        });
        if let Some(package) = matching_package {
            installed.push(package.clone());
        } else {
            not_installed.push(spec.clone())
        }
    }
    Ok((not_installed, installed))
}

pub fn filter_installed(
    location: &InstallLocation<impl Deref<Target = Path>>,
    specs: &[RequestedSpec],
    compatible_tags: &[(String, String, String)],
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    match location {
        InstallLocation::Venv {
            venv_base,
            python_version,
        } => filter_installed_venv(specs, venv_base, *python_version).context(format!(
            "Failed to filter packages installed in the venv at {}",
            venv_base.display()
        )),
        InstallLocation::Monotrail { monotrail_root, .. } => {
            filter_installed_monotrail(specs, monotrail_root, &compatible_tags)
                .context("Failed to filter installed packages")
        }
    }
}

/// Installs all given specs
pub fn install_all(
    specs: &[RequestedSpec],
    location: &InstallLocation<LockedDir>,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
    background: bool,
) -> anyhow::Result<Vec<InstalledPackage>> {
    match specs {
        // If everything is already installed, return silently
        [] if background => Ok(vec![]),
        [spec] => {
            if let Some(source) = &spec.source {
                info!(
                    "Installing {} ({})",
                    spec.requested, source.resolved_reference
                );
            } else {
                info!("Installing {}", spec.requested);
            }
            let start = Instant::now();
            let (python_version, unique_version, tag) = download_and_install(
                spec,
                &location,
                compatible_tags,
                no_compile,
                &location.get_python(),
            )?;
            debug!(
                "Installed {} {} in {:.1}s",
                spec.name,
                unique_version,
                start.elapsed().as_secs_f32()
            );
            let installed_package = InstalledPackage {
                name: spec.normalized_name(),
                python_version,
                unique_version,
                tag,
            };
            Ok(vec![installed_package])
        }
        _ => {
            let pb = ProgressBar::new(specs.len() as u64).with_style(
                ProgressStyle::default_bar()
                    .template("Installing {bar} {pos:>3}/{len:3} {wide_msg}"),
            );
            let current: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let installed = specs
                .par_iter()
                .map(|spec| {
                    current.lock().unwrap().push(spec.name.clone());
                    pb.set_message(current.lock().unwrap().join(","));
                    if pb.is_hidden() {
                        if let Some(source) = &spec.source {
                            info!(
                                "Installing {} ({})",
                                spec.requested, source.resolved_reference
                            );
                        } else {
                            info!("Installing {}", spec.requested);
                        }
                    }

                    let start = Instant::now();
                    let (python_version, unique_version, tag) = download_and_install(
                        spec,
                        &location,
                        compatible_tags,
                        no_compile,
                        &location.get_python(),
                    )?;
                    debug!(
                        "Installed {} {} in {:.1}s",
                        spec.name,
                        unique_version,
                        start.elapsed().as_secs_f32()
                    );
                    {
                        let mut current = current.lock().unwrap();
                        current.retain(|x| x != &spec.name);
                        pb.set_message(current.join(", "));
                        pb.inc(1);
                    }

                    let installed_package = InstalledPackage {
                        name: spec.normalized_name(),
                        python_version,
                        unique_version,
                        tag,
                    };
                    Ok(installed_package)
                })
                .collect::<Result<Vec<InstalledPackage>, anyhow::Error>>()?;
            pb.finish_and_clear();
            info!(
                "Installed {} packages in {:.1}s",
                pb.length(),
                pb.elapsed().as_secs_f32()
            );
            Ok(installed)
        }
    }
}

/// <https://stackoverflow.com/a/67240436/3549270>
fn checkout_revision(revision: &str, repo: Repository) -> Result<(), git2::Error> {
    let (object, reference) = repo.revparse_ext(revision)?;

    repo.checkout_tree(&object, None)?;

    match reference {
        // gref is an actual reference like branches or tags
        Some(gref) => repo.set_head(gref.name().unwrap()),
        // this is a commit, not a reference
        None => repo.set_head_detached(object.id()),
    }?;
    Ok(())
}

/// If the folder does not yet exist, it clones the repo and checks out the revision, otherwise
/// it fetches and checks out
pub fn repo_at_revision(url: &str, revision: &str, repo_dir: &Path) -> anyhow::Result<()> {
    let repo = if repo_dir.is_dir() {
        match Repository::open(repo_dir) {
            Ok(repo) => Some(repo),
            Err(err) => {
                warn!("Repository directory {} exists, but can't be opened as a git repository, recreating: {}", repo_dir.display(), err);
                fs::remove_dir_all(&repo_dir).context("Failed to remove old repo dir")?;
                None
            }
        }
    } else {
        None
    };
    let repo = if let Some(repo) = repo {
        let mut origin = repo
            .find_remote("origin")
            .context("No remote origin in repository")?;
        // required for default_branch
        origin
            .connect(Direction::Fetch)
            .context("Couldn't connect to remote")?;
        let default_branch = origin
            .default_branch()
            .context("Missing default branch")?
            .as_str()
            .context("Can't get default branch name")?
            .to_string();
        origin
            .fetch(&[default_branch], None, None)
            .context("Failed to fetch repository")?;
        drop(origin);
        repo
    } else {
        // We need to first clone the entire thing and then checkout the revision we want
        // https://stackoverflow.com/q/3489173/3549270
        let mut tries = 3;
        loop {
            let result = Repository::clone(url, &repo_dir);
            let backoff = Duration::from_secs(1);
            tries -= 1;
            match result {
                Ok(repository) => break repository,
                Err(err) if tries > 0 => {
                    warn!(
                        "Failed to clone {}, sleeping {}s and retrying: {}",
                        url,
                        err,
                        backoff.as_secs()
                    );
                    sleep(backoff);
                    continue;
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("Failed to clone {} to often", url));
                }
            };
        }
    };
    checkout_revision(revision, repo)
        .with_context(|| format!("failed to checkout revision {} for {}", revision, url))?;
    Ok(())
}

/// Returns the python version, unique version
fn download_and_install(
    requested_spec: &RequestedSpec,
    location: &InstallLocation<LockedDir>,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
    sys_executable: &Path,
) -> anyhow::Result<(String, String, String)> {
    let spec = requested_spec.resolve(compatible_tags)?;
    trace!("requested: {:?}, resolved: {:?}", requested_spec, spec);

    let (wheel_path, distribution_type) = match spec.location.clone() {
        FileOrUrl::File(file_path) => {
            if file_path.as_os_str().to_string_lossy().ends_with(".whl") {
                (file_path, DistributionType::Wheel)
            } else if file_path.as_os_str().to_string_lossy().ends_with(".tar.gz") {
                (file_path, DistributionType::SourceDistribution)
            } else {
                bail!(
                    "Unknown filetype (neither .whl not .tar.gz): {}",
                    file_path.display()
                )
            }
        }
        FileOrUrl::Url { url, filename } => {
            let wheel_path =
                download_distribution_cached(&spec.name, &spec.unique_version, &filename, &url)
                    .with_context(|| format!("Failed to download {} from pypi", spec.requested))?;

            (wheel_path, spec.distribution_type.clone())
        }
        FileOrUrl::Git { url, revision } => {
            let temp_dir = TempDir::new()?;
            let repo_dir = temp_dir.path().join(&spec.name);
            repo_at_revision(&url, &revision, &repo_dir)?;

            // If we got an sdist until now, build it into a wheel
            debug!(
                "Building {} {} from source distribution to wheel",
                spec.name, spec.unique_version
            );
            let wheel_path = build_source_distribution_to_wheel_cached(
                &spec.name,
                &spec.unique_version,
                &repo_dir,
                compatible_tags,
            )
            .with_context(|| {
                format!(
                    "Failed to build wheel from source for {} (repository: {} revision: {})",
                    spec.name, url, revision
                )
            })?;

            (wheel_path, DistributionType::Wheel)
        }
    };

    let wheel_path = if distribution_type == DistributionType::Wheel {
        wheel_path
    } else {
        // If we got an sdist until now, build it into a wheel
        debug!(
            "Building {} {} from source distribution to wheel",
            spec.name, spec.unique_version
        );
        build_source_distribution_to_wheel_cached(
            &spec.name,
            &spec.unique_version,
            &wheel_path,
            compatible_tags,
        )
        .with_context(|| {
            format!(
                "Failed to build wheel from source distribution for {}",
                wheel_path.display()
            )
        })?
    };
    debug!("Installing {} {}", spec.name, spec.unique_version);
    let tag = install_wheel(
        location,
        &wheel_path,
        !no_compile,
        &spec.extras,
        &spec.unique_version,
        &sys_executable,
    )
    .with_context(|| format!("Failed to install {}", spec.requested))?;
    Ok((spec.python_version, spec.unique_version, tag))
}
