use crate::install_location::InstallLocation;
use crate::markers::Pep508Environment;
use crate::package_index::download_distribution;
use crate::poetry::read_poetry_specs;
use crate::spec::RequestedSpec;
use crate::venv_parser::get_venv_python_version;
use crate::virtual_sprawl::{filter_installed, virtual_sprawl_root};
use crate::wheel_tags::current_compatible_tags;
use crate::{install_specs, package_index, WheelInstallerError};
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
        pyproject_toml: Option<PathBuf>,
        #[clap(long)]
        no_compile: bool,
        #[clap(long)]
        no_dev: bool,
        #[clap(long, short = 'E')]
        extras: Vec<String>,
        #[clap(long)]
        virtual_sprawl: bool,
        /// Only relevant for venv install
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
    let venv_base = venv.canonicalize()?;

    match cli {
        Cli::Install {
            targets,
            no_compile,
        } => {
            let compatible_tags = current_compatible_tags(venv)?;
            let specs = targets
                .iter()
                .map(|target| RequestedSpec::from_requested(target, &[]))
                .collect::<Result<Vec<RequestedSpec>, WheelInstallerError>>()?;
            let installation_location = InstallLocation::Venv {
                venv_base,
                python_version,
            };

            install_specs(
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
            // TODO: don't parse this from a subprocess but do it like maturin
            let pep508_env = Pep508Environment::from_python();
            let specs = read_poetry_specs(
                &pyproject_toml.unwrap_or_else(|| PathBuf::from("pyproject.toml")),
                no_dev,
                &extras,
                &pep508_env,
            )?;

            if virtual_sprawl {
                let virtual_sprawl_root = virtual_sprawl_root()?;
                let location = InstallLocation::VirtualSprawl {
                    virtual_sprawl_root: virtual_sprawl_root.clone(),
                    python: venv_base.join("bin").join("python"),
                    python_version,
                };

                let (to_install_specs, _installed_done) =
                    filter_installed(&specs, Path::new(&virtual_sprawl_root))?;
                install_specs(
                    &to_install_specs,
                    &location,
                    &compatible_tags,
                    no_compile,
                    false,
                )?;
            } else {
                let installation_location = InstallLocation::Venv {
                    venv_base: venv.canonicalize()?,
                    python_version,
                };

                let specs = if skip_existing {
                    specs
                        .into_iter()
                        .filter(|spec| {
                            let version = match spec.get_unique_version() {
                                None => {
                                    panic!("lockfile specs must have a version")
                                }
                                Some(version) => version,
                            };
                            !installation_location.is_installed(&spec.normalized_name(), &version)
                        })
                        .collect()
                } else {
                    specs
                };
                install_specs(
                    &specs,
                    &installation_location,
                    &compatible_tags,
                    no_compile,
                    false,
                )?;
            }
        }
    };
    Ok(())
}
