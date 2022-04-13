use crate::cli::download_distribution_cached;
use crate::install_location::InstallLocation;
use crate::install_wheel;
use crate::source_distribution::build_source_distribution_to_wheel_cached;
use crate::spec::{DistributionType, FileOrUrl, RequestedSpec};
use anyhow::{bail, Context};
use git2::Repository;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tracing::{debug, info, trace};

/// Naively returns name and version which is sufficient for the current system
/// Returns name, python version, unique version
pub fn install_specs(
    specs: &[RequestedSpec],
    location: &InstallLocation,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
    background: bool,
) -> anyhow::Result<Vec<(String, String, String)>> {
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
            let (python_version, unique_version) =
                download_and_install(spec, location, compatible_tags, no_compile)?;
            debug!("Installed {} {}", spec.name, unique_version);
            Ok(vec![(spec.name.clone(), python_version, unique_version)])
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

                    let (python_version, unique_version) =
                        download_and_install(spec, location, compatible_tags, no_compile)?;
                    debug!("Installed {} {}", spec.name, unique_version);
                    {
                        let mut current = current.lock().unwrap();
                        current.retain(|x| x != &spec.name);
                        pb.set_message(current.join(", "));
                        pb.inc(1);
                    }

                    Ok((spec.name.clone(), python_version, unique_version))
                })
                .collect::<Result<Vec<_>, anyhow::Error>>()?;
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
    location: &InstallLocation,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
) -> anyhow::Result<(String, String)> {
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
    let unique_id = format!("{}-{}", spec.normalized_name(), spec.unique_version);
    install_wheel(location, &wheel_path, !no_compile, &spec.extras, &unique_id)
        .with_context(|| format!("Failed to install {}", spec.requested))?;
    Ok((spec.python_version, spec.unique_version))
}

/// https://stackoverflow.com/a/67240436/3549270
fn checkout_revision(revision: &String, repo: Repository) -> Result<(), git2::Error> {
    let (object, reference) = repo.revparse_ext(&revision)?;

    repo.checkout_tree(&object, None)?;

    match reference {
        // gref is an actual reference like branches or tags
        Some(gref) => repo.set_head(gref.name().unwrap()),
        // this is a commit, not a reference
        None => repo.set_head_detached(object.id()),
    }?;
    Ok(())
}
