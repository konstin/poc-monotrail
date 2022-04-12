use crate::cli::download_distribution_cached;
use crate::install_location::InstallLocation;
use crate::install_wheel;
use crate::source_distribution::build_source_distribution_to_wheel_cached;
use crate::spec::{DistributionType, FileOrUrl, RequestedSpec};
use anyhow::{bail, Context};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, trace};

/// Naively returns name and version which is sufficient for the current system
pub fn install_specs(
    specs: &[RequestedSpec],
    location: &InstallLocation,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
    background: bool,
) -> anyhow::Result<Vec<(String, String)>> {
    match specs {
        // silent with we do python preload and have nothing to do
        [] if background => Ok(vec![]),
        [spec] => {
            info!("Installing {}", spec.requested);
            let unique_version = download_and_install(spec, location, compatible_tags, no_compile)?;
            debug!("Installed {} {}", spec.name, unique_version);
            Ok(vec![(spec.name.clone(), unique_version)])
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

                    let unique_version =
                        download_and_install(spec, location, compatible_tags, no_compile)?;
                    debug!("Installed {} {}", spec.name, unique_version);
                    {
                        let mut current = current.lock().unwrap();
                        current.retain(|x| x != &spec.name);
                        pb.set_message(current.join(", "));
                        pb.inc(1);
                    }

                    Ok((spec.name.clone(), unique_version))
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

/// Returns the version
fn download_and_install(
    requested_spec: &RequestedSpec,
    location: &InstallLocation,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
) -> anyhow::Result<String> {
    let spec = requested_spec.resolve(compatible_tags)?;
    trace!("requested: {:?}, resolved: {:?}", requested_spec, spec);

    let (wheel_path, distribution_type) = match spec.location {
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

            (wheel_path, spec.distribution_type)
        }
    };

    let wheel_path = if distribution_type == DistributionType::Wheel {
        wheel_path
    } else {
        // If we got an sdist until now, build it into a wheel
        debug!(
            "Build {} {} from source distribution to wheel",
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
    let unique_id = format!("{}-{}", spec.name, spec.unique_version);
    install_wheel(location, &wheel_path, !no_compile, &spec.extras, &unique_id)
        .with_context(|| format!("Failed to install {}", spec.requested))?;
    Ok(spec.unique_version)
}
