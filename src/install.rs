use crate::cli::download_distribution_cached;
use crate::install_location::{InstallLocation, LockedDir};
use crate::install_wheel;
use crate::source_distribution::build_source_distribution_to_wheel_cached;
use crate::spec::{DistributionType, FileOrUrl, RequestedSpec};
use anyhow::{bail, Context};
use git2::Repository;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tracing::{debug, info, trace};

/// what we communicate back to python
#[cfg(not(feature = "python_bindings"))]
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Naively returns name and version which is sufficient for the current system
/// Returns name, python version, unique version
pub fn install_specs(
    specs: &[RequestedSpec],
    location: &InstallLocation<PathBuf>,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
    background: bool,
) -> anyhow::Result<Vec<InstalledPackage>> {
    // Lock install directory to prevent races between multiple virtual sprawl porcesses
    // Lock it here instead of install_wheel to allow multithreading, since we'll only install
    // disjoint packages
    let location = location.acquire_lock()?;

    match specs {
        // silent with we do python preload and have nothing to do
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
            let (python_version, unique_version, tag) =
                download_and_install(spec, &location, compatible_tags, no_compile)?;
            debug!("Installed {} {}", spec.name, unique_version);
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
                ProgressStyle::default_bar().template("Installing {bar} {pos:>3}/{len:3} {msg}"),
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

                    let (python_version, unique_version, tag) =
                        download_and_install(spec, &location, compatible_tags, no_compile)?;
                    debug!("Installed {} {}", spec.name, unique_version);
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

/// Returns the python version, unique version
fn download_and_install(
    requested_spec: &RequestedSpec,
    location: &InstallLocation<LockedDir>,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
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
            // TODO: Lookup size
            debug!(
                "Downloading (or getting from cache) {} {}",
                spec.name, spec.unique_version
            );
            let wheel_path =
                download_distribution_cached(&spec.name, &spec.unique_version, &filename, &url)
                    .with_context(|| format!("Failed to download {} from pypi", spec.requested))?;

            (wheel_path, spec.distribution_type.clone())
        }
        FileOrUrl::Git { url, revision } => {
            let temp_dir = TempDir::new()?;
            let repo_dir = temp_dir.path().join(&spec.name);
            // We need to first clone the entire thing and then checkout the revision we want
            // https://stackoverflow.com/q/3489173/3549270
            let repo = Repository::clone(&url, &repo_dir)
                .with_context(|| format!("Failed to clone {}", url))?;
            checkout_revision(&revision, repo)
                .with_context(|| format!("failed to checkout revision {} for {}", revision, url))?;

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
    )
    .with_context(|| format!("Failed to install {}", spec.requested))?;
    Ok((spec.python_version, spec.unique_version, tag))
}

/// https://stackoverflow.com/a/67240436/3549270
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
