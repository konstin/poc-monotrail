use crate::install::InstalledPackage;
use crate::install_location::InstallLocation;
use crate::markers::Pep508Environment;
use crate::monotrail::monotrail_root;
use crate::package_index::download_distribution;
use crate::poetry::read_poetry_specs;
use crate::spec::RequestedSpec;
use crate::venv_parser::get_venv_python_version;
use crate::wheel_tags::current_compatible_tags;
use crate::{install_specs, package_index, WheelInstallerError};
use anyhow::{bail, format_err, Context};
use clap::Parser;
use fs_err::File;
use nix::unistd;
use std::env;
use std::ffi::CString;
use std::io::Read;
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
    monotrail: bool,
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
        command: String,
        #[clap(last = true)]
        command_args: Vec<String>,
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
    options: &PoetryOptions,
) -> anyhow::Result<(InstallLocation<PathBuf>, Vec<InstalledPackage>)> {
    let compatible_tags = current_compatible_tags(venv)?;
    // TODO: don't parse this from a subprocess but do it like maturin
    let pep508_env = Pep508Environment::from_python();
    let specs = read_poetry_specs(
        Path::new("pyproject.toml"),
        options.no_dev,
        &options.extras,
        &pep508_env,
    )?;

    let location = if options.monotrail {
        let monotrail_root = monotrail_root()?;
        InstallLocation::Monotrail {
            monotrail_root,
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

    let (to_install, mut installed_done) = if options.skip_existing || options.monotrail {
        location.filter_installed(&specs)?
    } else {
        (specs, Vec::new())
    };
    let mut installed_new = install_specs(
        &to_install,
        &location,
        &compatible_tags,
        options.no_compile,
        false,
    )?;
    installed_done.append(&mut installed_new);
    Ok((location, installed_done))
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
        Cli::PoetryInstall { options } => {
            install_location_specs(venv, python_version, &venv_canon, &options)?;
        }
        Cli::PoetryRun {
            options,
            command,
            command_args,
        } => {
            let (location, installed) =
                install_location_specs(venv, python_version, &venv_canon, &options)?;
            let executable = match location {
                // Using monotrail as launcher is kinda pointless when we're already in a venv ¯\_(ツ)_/¯
                InstallLocation::Venv { venv_base, .. } => {
                    let bin_dir = venv_base.join("bin");
                    let executable = bin_dir.join(&command);
                    if !executable.is_file() {
                        bail!("No such command '{}' in {}", command, bin_dir.display());
                    }
                    executable
                }
                InstallLocation::Monotrail { monotrail_root, .. } => installed
                    .iter()
                    .map(|installed_package| {
                        monotrail_root
                            .join(&installed_package.name)
                            .join(&installed_package.unique_version)
                            .join(&installed_package.tag)
                            .join("bin")
                            .join(&command)
                    })
                    .find(|candidate| candidate.is_file())
                    .with_context(|| {
                        format_err!("Couldn't find command {} in installed packages", command)
                    })?,
            };

            // Check whether we're launching a monotrail python script
            let mut executable_file = File::open(&executable)
                .context("the executable file was right there and is now unreadable ಠ_ಠ")?;
            let placeholder_python = b"#!python";
            // scripts might be binaries, so we read an exact number of bytes instead of the first line as string
            let mut start = Vec::new();
            start.resize(placeholder_python.len(), 0);
            executable_file.read_exact(&mut start)?;
            if start == placeholder_python {
                todo!()
            }

            // Sorry for the to_string_lossy
            // https://stackoverflow.com/a/38948854/3549270
            let executable_c_str = CString::new(executable.to_string_lossy().as_bytes())
                .context("Failed to convert executable path")?;
            let args_c_string = command_args
                .iter()
                .map(|arg| {
                    CString::new(arg.as_bytes()).context("Failed to convert executable argument")
                })
                .collect::<anyhow::Result<Vec<CString>>>()?;

            env::set_var("MONOTRAIL", "1");
            env::set_var("MONOTRAIL_CWD", env::current_dir()?.as_os_str());

            debug!("launching (execv) {}", executable.display());

            // We replace the current process with the new process is it's like actually just running
            // the real thing
            // note the that this may launch a python script, a native binary or anything else
            unistd::execv(&executable_c_str, &args_c_string).context("Failed to launch process")?;
        }
    };
    Ok(())
}
