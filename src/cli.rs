use crate::install_location::InstallLocation;
use crate::package_index::download_distribution;
use crate::poetry::find_specs_to_install;
use crate::spec::Spec;
use crate::venv_parser::get_venv_python_version;
use crate::wheel_tags::current_compatible_tags;
use crate::{install, package_index, WheelInstallerError};
use clap::Parser;
use std::path::{Path, PathBuf};
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
        #[clap(long)]
        virtual_sprawl: bool,
    },
}

/// Builds cache filename, downloads if not present, returns cache filename
pub fn download_distribution_cached(
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
    download_distribution(url, &target_dir, &target_file)?;

    Ok(target_file)
}

pub fn run(cli: Cli, venv: &Path) -> anyhow::Result<()> {
    let python_version = get_venv_python_version(venv)?;
    let installation_location = InstallLocation::Venv {
        venv_base: venv.to_path_buf(),
        python_version,
    };

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
            install::install_specs(&specs, &installation_location, &compatible_tags, no_compile)?;
        }
        Cli::PoetryInstall {
            pyproject_toml,
            no_compile,
            no_dev,
            extras,
            virtual_sprawl,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs =
                find_specs_to_install(&pyproject_toml, &compatible_tags, no_dev, &extras, None)?;

            let location = if virtual_sprawl {
                InstallLocation::VirtualSprawl {
                    virtual_sprawl_root: PathBuf::from("virtual_sprawl"),
                    python: installation_location.get_python()?,
                    python_version,
                }
            } else {
                installation_location
            };

            install::install_specs(&specs, &location, &compatible_tags, no_compile)?;
        }
    };
    Ok(())
}
