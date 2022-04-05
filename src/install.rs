use crate::install_location::InstallLocation;
use crate::install_wheel;
use crate::package_index::search_package;
use crate::spec::{DistributionType, Spec};
use anyhow::{bail, Context};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::cli::download_distribution_cached;
use crate::source_distribution::build_source_distribution_to_wheel_cached;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Naively returns name and version which is sufficient for the current system
pub fn install_specs(
    specs: &[Spec],
    location: &InstallLocation,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
) -> anyhow::Result<Vec<(String, String)>> {
    match specs {
        [spec] => {
            info!("Installing {}", spec.requested);
            let version = download_and_install(location, &compatible_tags, no_compile, spec)?;
            debug!("Installed {} {}", spec.name, version);
            Ok(vec![(spec.name.clone(), version)])
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
                        info!("Installing {}", spec.requested);
                    }

                    let version =
                        download_and_install(location, &compatible_tags, no_compile, spec)?;
                    debug!("Installed {} {}", spec.name, version);
                    {
                        let mut current = current.lock().unwrap();
                        current.retain(|x| x != &spec.name);
                        pb.set_message(current.join(", "));
                        pb.inc(1);
                    }

                    Ok((spec.name.clone(), version))
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

fn download_and_install(
    location: &InstallLocation,
    compatible_tags: &&[(String, String, String)],
    no_compile: bool,
    spec: &Spec,
) -> anyhow::Result<String> {
    let (wheel_path, distribution_type, version) = match &spec.file_path {
        // case 1: we already got a file (wheel or sdist)
        Some((file_path, metadata)) => {
            if file_path.as_os_str().to_string_lossy().ends_with(".whl") {
                (
                    file_path.to_owned(),
                    DistributionType::Wheel,
                    metadata.version.clone(),
                )
            } else if file_path.as_os_str().to_string_lossy().ends_with(".tar.gz") {
                (
                    file_path.to_owned(),
                    DistributionType::SourceDistribution,
                    metadata.version.clone(),
                )
            } else {
                bail!(
                    "Unknown filetype (neither .whl not .tar.gz): {}",
                    file_path.display()
                )
            }
        }
        None => {
            let (url, filename, distribution_type, version) = match spec.clone() {
                // case 2: we have version and url
                Spec {
                    version: Some(version),
                    url: Some((url, filename, distribution_type)),
                    ..
                } => (url, filename, distribution_type, version),
                // case 3: we have a name and maybe a version -> search fitting version and url
                _ => search_package(&spec.name, spec.version.as_deref(), compatible_tags)?,
            };
            // TODO: Lookup size
            debug!(
                "Downloading (or getting from cache) {} {}",
                spec.name, version
            );
            let wheel_path = download_distribution_cached(&spec.name, &version, &filename, &url)
                .with_context(|| format!("Failed to download {} from pypi", spec.requested))?;
            (wheel_path, distribution_type, version)
        }
    };
    let wheel_path = if distribution_type == DistributionType::Wheel {
        wheel_path
    } else {
        // If we got an sdist until now, build it into a wheel
        debug!(
            "Build {} {} from source distribution to wheel",
            spec.name, version
        );
        build_source_distribution_to_wheel_cached(
            &spec.name,
            &version,
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
    debug!("Installing {} {}", spec.name, version);
    install_wheel(location, &wheel_path, !no_compile).with_context(|| {
        if let Some((file_path, _)) = &spec.file_path {
            format!("Failed to install {}", file_path.display())
        } else {
            format!("Failed to install {} {}", spec.name, version)
        }
    })?;
    Ok(version)
}
