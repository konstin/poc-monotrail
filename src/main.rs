use crate::install::install_wheel;
use anyhow::{bail, Context};
use clap::Parser;
use rayon::prelude::*;
use std::env;
use std::path::PathBuf;
use tracing::{debug, info};

mod install;
#[cfg(feature = "package_index")]
mod package_index;

#[derive(Parser)]
enum Cli {
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

fn run() -> anyhow::Result<()> {
    let cli: Cli = Cli::parse();
    debug!("VIRTUAL_ENV: {:?}", env::var_os("VIRTUAL_ENV"));
    let venv_base = if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
        PathBuf::from(virtual_env)
    } else {
        bail!("Will only install in a virtualenv");
    };
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
                let (name, version) = install_wheel(&venv_base, file, !no_compile)
                    .with_context(|| format!("Failed to install {}", file.display()))?;
                info!("Installed {} {}", name, version);
            }
            _ => {
                files
                    .par_iter()
                    .map(|file| {
                        println!("Installing {}", file.display());
                        let (name, version) = install::install_wheel(&venv_base, file, !no_compile)
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

fn main() {
    // Good enough for now
    if env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt::init();
    } else {
        let format = tracing_subscriber::fmt::format()
            .with_level(false)
            .with_target(false)
            .without_time()
            .compact();
        tracing_subscriber::fmt().event_format(format).init();
    }
    if let Err(e) = run() {
        eprintln!("💥 {} failed", env!("CARGO_PKG_NAME"));
        for cause in e.chain().collect::<Vec<_>>().iter() {
            eprintln!("  Caused by: {}", cause);
        }
        std::process::exit(1);
    }
}
