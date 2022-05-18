use crate::inject_and_run::{inject_and_run_python, parse_major_minor, run_from_python_args};
use crate::install::{filter_installed, InstalledPackage};
use crate::markers::Pep508Environment;
use crate::monotrail::{
    find_scripts, install_specs_to_finder, monotrail_root, LaunchType, PythonContext,
};
use crate::package_index::download_distribution;
use crate::poetry_integration::read_dependencies::{read_poetry_specs, read_toml_files};
use crate::poetry_integration::run::poetry_run;
use crate::spec::RequestedSpec;
use crate::standalone_python::provision_python;
use crate::venv_parser::get_venv_python_version;
use crate::{get_specs, install_specs, monotrail, package_index};
use anyhow::{bail, format_err, Context};
use clap::Parser;
use install_wheel_rs::{compatible_tags, Arch, InstallLocation, Os, WheelInstallerError};
use nix::unistd;
use std::env;
use std::ffi::CString;
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
    /// Like `python <...>`, but installs the dependencies before
    ///
    /// If you run python with a script, e.g. `python my/files/script.py`, monotrail will look for
    /// dependency specification (pyproject.toml or requirements.txt) next to script.py and up
    /// the file system tree.
    RunPython { python_args: Vec<String> },
    /// Runs one of the scripts that would be available if you were using an activated venv.
    /// (the contents of .venv/bin/ on linux/mac)
    RunScript {
        #[clap(long, short = 'E')]
        extras: Vec<String>,
        #[clap(long, short)]
        python_version: Option<String>,
        script: String,
        #[clap(last = true)]
        script_args: Vec<String>,
    },
    /// Run the poetry bundled with monotrail
    #[clap(trailing_var_arg = true)]
    Poetry { args: Vec<String> },
    VenvInstall {
        targets: Vec<String>,
        #[clap(long)]
        no_compile: bool,
    },
    /// Faster reimplementation of "poetry install" for both venvs and monotrail
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
    let compatible_tags = compatible_tags(
        get_venv_python_version(venv)?,
        &Os::current()?,
        &Arch::current()?,
    )?;
    // TODO: don't parse this from a subprocess but do it like maturin
    let pep508_env = Pep508Environment::from_python(Path::new("python"));
    let (poetry_toml, poetry_lock, _lockfile) = read_toml_files(&env::current_dir()?)?;
    let specs = read_poetry_specs(
        poetry_toml,
        poetry_lock,
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
        filter_installed(&location, &specs)?
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

pub fn run(cli: Cli, venv: Option<&Path>) -> anyhow::Result<()> {
    match cli {
        Cli::RunScript {
            extras,
            python_version,
            script,
            script_args,
        } => {
            let python_version = parse_major_minor(python_version.as_deref().unwrap_or("3.8"))?;
            let python_root = provision_python(python_version)?;
            let python_binary = python_root.join("install").join("bin").join("python3");
            let pep508_env = Pep508Environment::from_python(&python_binary);
            let python_context = PythonContext {
                sys_executable: python_binary.clone(),
                python_version,
                pep508_env,
                launch_type: LaunchType::Binary,
            };
            let (specs, wrong_scripts, lockfile) = get_specs(None, &extras, &python_context)?;
            let finder_data =
                install_specs_to_finder(&specs, wrong_scripts, lockfile, None, &python_context)?;

            let script_path = find_scripts(
                &finder_data.sprawl_packages,
                Path::new(&finder_data.sprawl_root),
            )
            .context("Failed to collect scripts")?
            .get(&script)
            .with_context(|| format_err!("Couldn't find command {} in installed packages", script))?
            .to_path_buf();

            let is_python_script = monotrail::is_python_script(&script_path)?;

            if is_python_script {
                debug!("launching (python) {}", script_path.display());
                let args: Vec<String> = [
                    python_binary.to_string_lossy().to_string(),
                    script_path.to_string_lossy().to_string(),
                ]
                .into_iter()
                .chain(script_args)
                .collect();
                inject_and_run_python(
                    &python_root,
                    &args,
                    &serde_json::to_string(&finder_data).unwrap(),
                )?;
            } else {
                // Sorry for the to_string_lossy all over the place
                // https://stackoverflow.com/a/38948854/3549270
                let executable_c_str = CString::new(script_path.to_string_lossy().as_bytes())
                    .context("Failed to convert executable path")?;
                let args_c_string = script_args
                    .iter()
                    .map(|arg| {
                        CString::new(arg.as_bytes())
                            .context("Failed to convert executable argument")
                    })
                    .collect::<anyhow::Result<Vec<CString>>>()?;

                debug!("launching (execv) {}", script_path.display());
                // We replace the current process with the new process is it's like actually just running
                // the real thing
                // note the that this may launch a python script, a native binary or anything else
                unistd::execv(&executable_c_str, &args_c_string)
                    .context("Failed to launch process")?;
            }
        }
        Cli::VenvInstall {
            targets,
            no_compile,
        } => {
            let venv = if let Some(venv) = venv {
                venv.to_path_buf()
            } else if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
                PathBuf::from(virtual_env)
            } else {
                bail!("Will only install in a virtualenv");
            };
            let python_version = get_venv_python_version(&venv)?;
            let venv_canon = venv.canonicalize()?;

            let compatible_tags = compatible_tags(
                get_venv_python_version(&venv)?,
                &Os::current()?,
                &Arch::current()?,
            )?;
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
        Cli::RunPython { python_args } => {
            run_from_python_args(&python_args)?;
        }
        Cli::PoetryInstall { options } => {
            let venv = if let Some(venv) = venv {
                venv.to_path_buf()
            } else if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
                PathBuf::from(virtual_env)
            } else {
                let venv = PathBuf::from(".venv");
                if venv.join("pyvenv.cfg").is_file() {
                    venv
                } else {
                    bail!("No .venv directory found");
                }
            };
            let python_version = get_venv_python_version(&venv)?;
            let venv_canon = venv.canonicalize()?;
            install_location_specs(&venv, python_version, &venv_canon, &options)?;
        }
        Cli::PoetryRun {
            options,
            command,
            command_args,
        } => {
            let venv = if let Some(venv) = venv {
                venv.to_path_buf()
            } else if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
                PathBuf::from(virtual_env)
            } else {
                bail!("Will only install in a virtualenv");
            };
            let python_version = get_venv_python_version(&venv)?;
            let venv_canon = venv.canonicalize()?;

            let (location, installed) =
                install_location_specs(&venv, python_version, &venv_canon, &options)?;
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
                InstallLocation::Monotrail { monotrail_root, .. } => {
                    let scripts = find_scripts(&installed, &monotrail_root)
                        .context("Failed to collect scripts")?;
                    scripts
                        .get(&command)
                        .with_context(|| {
                            format_err!("Couldn't find command {} in installed packages", command)
                        })?
                        .to_path_buf()
                }
            };

            let is_python_script = monotrail::is_python_script(&executable)?;
            if is_python_script {
                todo!()
            }

            // Sorry for the to_string_lossy all over the place
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
        Cli::Poetry { args } => {
            poetry_run(args)?;
        }
    };
    Ok(())
}
