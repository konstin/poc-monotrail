use crate::install::get_name_from_path;
use crate::install_wheel;
#[cfg(feature = "package_index")]
use crate::package_index;
use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

#[derive(Parser)]
pub enum Cli {
    #[cfg(feature = "package_index")]
    Install {
        name: String,
        version: Option<String>,
        #[clap(long)]
        no_compile: bool,
    },
    InstallFiles {
        files: Vec<PathBuf>,
        #[clap(long)]
        no_compile: bool,
    },
}

pub fn run(cli: Cli, venv_base: &Path) -> anyhow::Result<()> {
    match cli {
        #[cfg(feature = "package_index")]
        Cli::Install {
            name,
            version,
            no_compile,
        } => {
            let wheel_path = package_index::download_wheel(&name, version.as_deref())
                .with_context(|| format!("Failed to download {} from pypi", name))?;
            let (name, version) = install_wheel(&venv_base, &wheel_path, !no_compile)?;
            info!("Installed {} {}", name, version);
        }
        Cli::InstallFiles { files, no_compile } => match files.as_slice() {
            [file] => {
                let (name, version) = install_wheel(venv_base, file, !no_compile)
                    .with_context(|| format!("Failed to install {}", file.display()))?;
                info!("Installed {} {}", name, version);
            }
            _ => {
                let pb = ProgressBar::new(files.len() as u64).with_style(
                    ProgressStyle::default_bar()
                        .template("Installing {bar} {pos:>3}/{len:3} {msg}"),
                );
                let current: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
                files
                    .par_iter()
                    .map(|file| {
                        let task_name = get_name_from_path(file)?;
                        current.lock().unwrap().push(task_name.clone());
                        pb.set_message(current.lock().unwrap().join(","));

                        debug!("Installing {}", file.display());
                        let (name, version) = install_wheel(venv_base, file, !no_compile)
                            .with_context(|| format!("Failed to install {}", file.display()))?;
                        debug!("Installed {} {}", name, version);
                        {
                            let mut current = current.lock().unwrap();
                            current.retain(|x| x != &task_name);
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
        },
    };
    Ok(())
}

/*
  Compiling strsim v0.10.0
  Compiling percent-encoding v2.1.0
  Compiling camino v1.0.7
  Compiling gimli v0.26.1
  Compiling crossbeam-utils v0.8.6
  Compiling semver v1.0.4
  Compiling itoa v1.0.1
  Compiling ppv-lite86 v0.2.16
  Compiling bitflags v1.3.2
  Building [===>                      ] 41/224: strsim, ppv-lite86, unicode-bidi, bitfl...
*/
