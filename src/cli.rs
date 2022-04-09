use crate::install_location::InstallLocation;
use crate::package_index::download_distribution;
use crate::poetry::poetry_lockfile_to_specs;
use crate::spec::Spec;
use crate::venv_parser::get_venv_python_version;
use crate::wheel_tags::current_compatible_tags;
use crate::{install, install_specs, package_index, WheelInstallerError};
use clap::Parser;
use std::path::{Path, PathBuf};
use tracing::debug;

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
        #[clap(long)]
        skip_existing: bool,
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

    debug!("Downloading (or getting from cache) {} {}", name, version);
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
                .map(|target| Spec::from_requested(target, Vec::new()))
                .collect::<Result<Vec<Spec>, WheelInstallerError>>()?;
            install::install_specs(
                &specs,
                &installation_location,
                &compatible_tags,
                no_compile,
                false,
            )?;
        }
        Cli::PoetryInstall {
            pyproject_toml,
            no_compile,
            no_dev,
            extras,
            virtual_sprawl,
            skip_existing,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs = poetry_lockfile_to_specs(&pyproject_toml, no_dev, &extras, None)?;

            let location = if virtual_sprawl {
                InstallLocation::VirtualSprawl {
                    virtual_sprawl_root: PathBuf::from("virtual_sprawl"),
                    python: installation_location.get_python()?,
                    python_version,
                }
            } else {
                installation_location
            };

            let specs = if skip_existing {
                specs
                    .into_iter()
                    .filter(|spec| {
                        !location.is_installed(&spec.name, &spec.version.clone().unwrap())
                    })
                    .collect()
            } else {
                specs
            };

            install_specs(&specs, &location, &compatible_tags, no_compile, false)?;
        }
    };
    Ok(())
}
