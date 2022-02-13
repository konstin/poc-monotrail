use crate::package_index::{download_wheel_cached, search_wheel};
use crate::poetry_lock::get_install_list;

use crate::wheel_tags::{current_compatible_tags, WheelFilename};
use crate::{install_wheel, WheelInstallerError};

use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::str::FromStr;
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

/// Returns user requested target, name, version, filepath
fn parse_spec(
    target: impl AsRef<str>,
) -> Result<(String, String, Option<String>, Option<PathBuf>), WheelInstallerError> {
    if target.as_ref().ends_with(".whl") {
        let file_path = PathBuf::from(target.as_ref());
        let filename = file_path
            .file_name()
            .ok_or_else(|| WheelInstallerError::InvalidWheel("Expected a file".to_string()))?
            .to_string_lossy();
        let metadata = WheelFilename::from_str(&filename)?;
        Ok((
            target.as_ref().to_string(),
            metadata.distribution,
            Some(metadata.version),
            Some(file_path),
        ))
    } else {
        // TODO: check actual naming rules
        let valid_name = Regex::new(r"[-_a-zA-Z0-9.]+").unwrap();
        if let Some((name, version)) = target.as_ref().split_once("==") {
            Ok((
                target.as_ref().to_string(),
                name.to_string(),
                Some(version.to_string()),
                None,
            ))
        } else if valid_name.is_match(target.as_ref()) {
            Ok((
                target.as_ref().to_string(),
                target.as_ref().to_string(),
                None,
                None,
            ))
        } else {
            Err(WheelInstallerError::Pep440)
        }
    }
}

pub fn run(cli: Cli, venv: &Path) -> anyhow::Result<()> {
    match cli {
        Cli::Install {
            targets,
            no_compile,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs = targets.iter().map(parse_spec).collect::<Result<
                Vec<(String, String, Option<String>, Option<PathBuf>)>,
                WheelInstallerError,
            >>()?;
            match specs.as_slice() {
                [(target, name, version, file_path)] => {
                    let wheel_path = match file_path {
                        Some(file_path) => file_path.to_owned(),
                        None => {
                            let (url, filename, version) =
                                search_wheel(name, version.as_deref(), &compatible_tags)?;
                            // TODO: Lookup size
                            info!("Downloading {} {}", name, version);
                            download_wheel_cached(name, &version, &filename, &url).with_context(
                                || format!("Failed to download {} from pypi", target),
                            )?
                        }
                    };
                    let (name, version) = install_wheel(venv, &wheel_path, !no_compile)
                        .with_context(|| {
                            if let Some(file_path) = file_path {
                                format!("Failed to install {}", file_path.display())
                            } else {
                                format!(
                                    "Failed to install {} {}",
                                    name,
                                    version.as_deref().unwrap_or("")
                                )
                            }
                        })?;
                    info!("Installed {} {}", name, version);
                }
                _ => {
                    let pb = ProgressBar::new(targets.len() as u64).with_style(
                        ProgressStyle::default_bar()
                            .template("Installing {bar} {pos:>3}/{len:3} {msg}"),
                    );
                    let current: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
                    specs
                        .par_iter()
                        .map(|(target, name, version, file_path)| {
                            current.lock().unwrap().push(name.clone());
                            pb.set_message(current.lock().unwrap().join(","));
                            if pb.is_hidden() {
                                pb.println(&name);
                            }

                            let wheel_path = match file_path {
                                Some(file_path) => file_path.to_owned(),
                                None => {
                                    let (url, filename, version) =
                                        search_wheel(name, version.as_deref(), &compatible_tags)?;
                                    // TODO: Lookup size
                                    info!("Downloading {} {}", name, version);
                                    download_wheel_cached(name, &version, &filename, &url)
                                        .with_context(|| {
                                            format!("Failed to download {} from pypi", target)
                                        })?
                                }
                            };
                            debug!("Installing {} {:?}", name, version);
                            let (name, version) = install_wheel(venv, &wheel_path, !no_compile)
                                .with_context(|| {
                                    if let Some(file_path) = file_path {
                                        format!("Failed to install {}", file_path.display())
                                    } else {
                                        format!(
                                            "Failed to install {} {}",
                                            name,
                                            version.as_deref().unwrap_or("")
                                        )
                                    }
                                })?;
                            info!("Installed {} {} ({})", name, version, target);
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
        }
        Cli::PoetryInstall {
            lockfile,
            no_compile: _,
        } => {
            get_install_list(&lockfile)?;
        }
    };
    Ok(())
}
