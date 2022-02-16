use crate::package_index::{download_wheel_cached, search_wheel};
use crate::poetry_lock::specs_from_lockfile;
use crate::spec::Spec;
use crate::wheel_tags::current_compatible_tags;
use crate::{install_wheel, WheelInstallerError};
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
        lockfile: PathBuf,
        #[clap(long)]
        no_compile: bool,
    },
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
                        pb.println(&spec.requested);
                    }

                    let (wheel_path, version) = match &spec.file_path {
                        Some((file_path, metadata)) => {
                            (file_path.to_owned(), metadata.version.clone())
                        }
                        None => {
                            let (url, filename, version) =
                                search_wheel(&spec.name, spec.version.as_deref(), compatible_tags)?;
                            // TODO: Lookup size
                            info!("Downloading {} {}", spec.name, version);
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
                    info!("Installed {} {}", name, version);
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
            lockfile,
            no_compile,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs = specs_from_lockfile(&lockfile, &compatible_tags)?;
            install_specs(&specs, venv, &compatible_tags, no_compile)?;
        }
    };
    Ok(())
}
