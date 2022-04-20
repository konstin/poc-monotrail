use crate::install::InstalledPackage;
use crate::install_location::InstallLocation;
use crate::markers::Pep508Environment;
use crate::package_index::download_distribution;
use crate::poetry::read_poetry_specs;
use crate::spec::RequestedSpec;
use crate::venv_parser::get_venv_python_version;
use crate::virtual_sprawl::virtual_sprawl_root;
use crate::wheel_tags::current_compatible_tags;
use crate::{install_specs, package_index, WheelInstallerError};
use clap::Parser;
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Parser)]
pub struct PoetryOptions {
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
}

#[derive(Parser)]
pub enum Cli {
    Install {
        targets: Vec<String>,
        #[clap(long)]
        no_compile: bool,
    },
    PoetryInstall {
        #[clap(flatten)]
        options: PoetryOptions,
    },
    PoetryRun {
        #[clap(flatten)]
        options: PoetryOptions,
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

fn install_location_specs(
    venv: &Path,
    python_version: (u8, u8),
    venv_canon: &Path,
    no_compile: bool,
    no_dev: bool,
    extras: &[String],
    virtual_sprawl: bool,
    skip_existing: bool,
) -> anyhow::Result<Vec<InstalledPackage>> {
    let compatible_tags = current_compatible_tags(venv)?;
    // TODO: don't parse this from a subprocess but do it like maturin
    let pep508_env = Pep508Environment::from_python();
    let specs = read_poetry_specs(Path::new("pyproject.toml"), no_dev, extras, &pep508_env)?;

    let location = if virtual_sprawl {
        let virtual_sprawl_root = virtual_sprawl_root()?;
        InstallLocation::VirtualSprawl {
            virtual_sprawl_root,
            python: venv_canon.join("bin").join("python"),
            python_version,
        }
    } else {
        let venv_base = venv.canonicalize()?;
        InstallLocation::Venv {
            venv_base,
            python_version,
        }
    };

    let (to_install, mut installed_done) = if skip_existing || virtual_sprawl {
        location.filter_installed(&specs)?
    } else {
        (specs, Vec::new())
    };
    let mut installed_new =
        install_specs(&to_install, &location, &compatible_tags, no_compile, false)?;
    installed_done.append(&mut installed_new);
    Ok(installed_done)
}

pub fn run(cli: Cli, venv: &Path) -> anyhow::Result<()> {
    let python_version = get_venv_python_version(venv)?;
    let venv_canon = venv.canonicalize()?;

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
                venv_base: venv_canon,
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
            options:
                PoetryOptions {
                    no_compile,
                    no_dev,
                    extras,
                    virtual_sprawl,
                    skip_existing,
                },
        } => {
            install_location_specs(
                venv,
                python_version,
                &venv_canon,
                no_compile,
                no_dev,
                &extras,
                virtual_sprawl,
                skip_existing,
            )?;
        }
        Cli::PoetryRun { options } => {
            let installed = install_location_specs(
                venv,
                python_version,
                &venv_canon,
                options.no_compile,
                options.no_dev,
                &options.extras,
                options.virtual_sprawl,
                options.skip_existing,
            )?;
            dbg!(installed);
            todo!()
        }
    };
    Ok(())
}
