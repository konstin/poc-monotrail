use crate::install_wheel;
#[cfg(feature = "package_index")]
use crate::package_index;
use anyhow::Context;
use clap::Parser;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::path::{Path, PathBuf};
use tracing::info;

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
                files
                    .par_iter()
                    .map(|file| {
                        info!("Installing {}", file.display());
                        let (name, version) = install_wheel(venv_base, file, !no_compile)
                            .with_context(|| format!("Failed to install {}", file.display()))?;
                        info!("Installed {} {}", name, version);
                        Ok(())
                    })
                    .collect::<Result<Vec<()>, anyhow::Error>>()?;
            }
        },
    };
    Ok(())
}
