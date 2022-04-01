use crate::package_index::{download_wheel, search_wheel};
use crate::poetry::find_specs_to_install;
use crate::spec::Spec;
use crate::wheel_tags::current_compatible_tags;
use crate::{install_wheel, package_index, WheelInstallerError};
use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

#[derive(Parser)]
pub enum Cli {
    Install {
        targets: Vec<String>,
        #[clap(long)]
        no_compile: bool,
    },
    PoetryInstall {
        pyproject_toml: PathBuf,
        #[clap(long)]
        no_compile: bool,
        #[clap(long)]
        no_dev: bool,
        #[clap(short = 'E')]
        extras: Vec<String>,
    },
}

/// Builds cache filename, downloads if not present, returns cache filename
pub fn download_wheel_cached(
    name: &str,
    version: &str,
    filename: &str,
    url: &str,
) -> anyhow::Result<PathBuf> {
    let target_dir = package_index::cache_dir()?
        .join("artifacts")
        .join(name)
        .join(version);
    let target_file = target_dir.join(&filename);

    if target_file.is_file() {
        debug!("Using cached download at {}", target_file.display());
        return Ok(target_file);
    }

    info!("Downloading (or getting from cache) {} {}", name, version);
    download_wheel(url, &target_dir, &target_file)?;

    Ok(target_file)
}

fn install_specs(
    specs: &[Spec],
    venv: &Path,
    compatible_tags: &[(String, String, String)],
    no_compile: bool,
) -> anyhow::Result<()> {
    match specs {
        [spec] => {
            let (wheel_path, version) = match &spec.file_path {
                Some((file_path, metadata)) => (file_path.to_owned(), metadata.version.clone()),
                None => {
                    let (url, filename, version) =
                        search_wheel(&spec.name, spec.version.as_deref(), compatible_tags)?;
                    // TODO: Lookup size
                    info!("Downloading {} {}", spec.name, version);
                    let wheel_path = download_wheel_cached(&spec.name, &version, &filename, &url)
                        .with_context(|| {
                        format!("Failed to download {} from pypi", spec.requested)
                    })?;
                    (wheel_path, version)
                }
            };
            let (name, version) =
                install_wheel(venv, &wheel_path, !no_compile).with_context(|| {
                    if let Some((file_path, _)) = &spec.file_path {
                        format!("Failed to install {}", file_path.display())
                    } else {
                        format!("Failed to install {} {}", spec.name, version)
                    }
                })?;
            info!("Installed {} {}", name, version);
        }
        _ => {
            let pb = ProgressBar::new(specs.len() as u64).with_style(
                ProgressStyle::default_bar().template("Installing {bar} {pos:>3}/{len:3} {msg}"),
            );
            let current: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            specs
                .par_iter()
                .map(|spec| {
                    current.lock().unwrap().push(spec.name.clone());
                    pb.set_message(current.lock().unwrap().join(","));
                    if pb.is_hidden() {
                        info!("Installing {}", spec.requested);
                    }

                    let (wheel_path, version) = match &spec.file_path {
                        Some((file_path, metadata)) => {
                            (file_path.to_owned(), metadata.version.clone())
                        }
                        None => {
                            let (url, filename, version) =
                                search_wheel(&spec.name, spec.version.as_deref(), compatible_tags)?;
                            // TODO: Lookup size
                            debug!(
                                "Downloading (or getting from cache) {} {}",
                                spec.name, version
                            );
                            let wheel_path =
                                download_wheel_cached(&spec.name, &version, &filename, &url)
                                    .with_context(|| {
                                        format!("Failed to download {} from pypi", spec.requested)
                                    })?;
                            (wheel_path, version)
                        }
                    };
                    debug!("Installing {} {:?}", spec.name, version);
                    let (name, version) = install_wheel(venv, &wheel_path, !no_compile)
                        .with_context(|| {
                            if let Some((file_path, _)) = &spec.file_path {
                                format!("Failed to install {}", file_path.display())
                            } else {
                                format!("Failed to install {} {}", spec.name, version)
                            }
                        })?;
                    debug!("Installed {} {}", name, version);
                    {
                        let mut current = current.lock().unwrap();
                        current.retain(|x| x != &name);
                        pb.set_message(current.join(", "));
                        pb.inc(1);
                    }

                    Ok(())
                })
                .collect::<Result<Vec<()>, anyhow::Error>>()?;
            pb.finish_and_clear();
            info!(
                "Installed {} packages in {:.1}s",
                pb.length(),
                pb.elapsed().as_secs_f32()
            );
        }
    }
    Ok(())
}

pub fn run(cli: Cli, venv: &Path) -> anyhow::Result<()> {
    match cli {
        Cli::Install {
            targets,
            no_compile,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs = targets
                .iter()
                .map(Spec::from_requested)
                .collect::<Result<Vec<Spec>, WheelInstallerError>>()?;
            install_specs(&specs, venv, &compatible_tags, no_compile)?;
        }
        Cli::PoetryInstall {
            pyproject_toml,
            no_compile,
            no_dev,
            extras,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs = find_specs_to_install(&pyproject_toml, &compatible_tags, no_dev, &extras)?;
            install_specs(&specs, venv, &compatible_tags, no_compile)?;
        }
    };
    Ok(())
}
