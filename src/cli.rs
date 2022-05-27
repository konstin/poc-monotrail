use crate::inject_and_run::{
    inject_and_run_python, parse_major_minor, prepare_execve_environment, run_python_args,
};
use crate::install::{filter_installed, InstalledPackage};
use crate::markers::Pep508Environment;
use crate::monotrail::{find_scripts, install_specs_to_finder, is_python_script, monotrail_root};
use crate::package_index::download_distribution;
use crate::poetry_integration::read_dependencies::{read_poetry_specs, read_toml_files};
use crate::poetry_integration::run::poetry_run;
use crate::spec::RequestedSpec;
use crate::standalone_python::provision_python;
use crate::venv_parser::get_venv_python_version;
use crate::{get_specs, install_specs, package_index};
use anyhow::{bail, format_err, Context};
use clap::Parser;
use install_wheel_rs::{compatible_tags, Arch, InstallLocation, Os, WheelInstallerError};
use nix::unistd;
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
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

#[derive(clap::Subcommand)]
pub enum RunSubcommand {
    #[clap(trailing_var_arg = true)]
    Python { python_args: Vec<String> },
    #[clap(trailing_var_arg = true)]
    Script {
        script: String,
        script_args: Vec<String>,
    },
}

#[derive(Parser)]
pub enum Cli {
    Run {
        /// Install those extras from pyproject.toml
        #[clap(long, short = 'E')]
        extras: Vec<String>,
        /// Run this python version x.y
        #[clap(long, short)]
        python_version: Option<String>,
        /// Directory with the pyproject.toml
        #[clap(long)]
        root: Option<PathBuf>,
        #[clap(subcommand)]
        action: RunSubcommand,
    },
    /// Like `python <...>`, but installs the dependencies before
    ///
    /// If you run python with a script, e.g. `python my/files/script.py`, monotrail will look for
    /// dependency specification (pyproject.toml or requirements.txt) next to script.py and up
    /// the file system tree.
    RunPython { python_args: Vec<String> },
    /// Runs one of the scripts that would be available if you were using an activated venv.
    /// (the contents of .venv/bin/ on linux/mac)
    #[clap(trailing_var_arg = true)]
    RunScript {
        #[clap(long, short = 'E')]
        extras: Vec<String>,
        #[clap(long, short)]
        python_version: Option<String>,
        script: String,
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
    let (poetry_section, poetry_lock, _lockfile) = read_toml_files(&env::current_dir()?)?;
    let specs = read_poetry_specs(
        &poetry_section,
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
        filter_installed(&location, &specs, &compatible_tags)?
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

pub fn run_cli(cli: Cli, venv: Option<&Path>) -> anyhow::Result<Option<i32>> {
    match cli {
        Cli::Run {
            extras,
            python_version,
            root,
            action,
        } => match action {
            RunSubcommand::Python { python_args } => Ok(Some(run_python_args(
                &python_args,
                python_version.as_deref(),
                root.as_deref(),
                &extras,
            )?)),
            RunSubcommand::Script {
                script,
                script_args,
            } => Ok(Some(run_script(
                &extras,
                python_version,
                &script,
                script_args,
            )?)),
        },
        Cli::RunPython { python_args } => Ok(Some(run_python_args(&python_args, None, None, &[])?)),
        Cli::RunScript {
            extras,
            python_version,
            script,
            script_args,
        } => Ok(Some(run_script(
            &extras,
            python_version,
            &script,
            script_args,
        )?)),
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
            Ok(None)
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
            Ok(None)
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

            let is_python_script = is_python_script(&executable)?;
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
            unreachable!()
        }
        Cli::Poetry { args } => Ok(Some(poetry_run(&args, None)?)),
    }
}

fn run_script(
    extras: &[String],
    python_version: Option<String>,
    script: &str,
    script_args: Vec<String>,
) -> anyhow::Result<i32> {
    let python_version = parse_major_minor(python_version.as_deref().unwrap_or("3.8"))?;
    let (python_context, python_home) = provision_python(python_version)?;

    let (specs, wrong_scripts, lockfile) = get_specs(None, extras, &python_context)?;
    let finder_data =
        install_specs_to_finder(&specs, wrong_scripts, lockfile, None, &python_context)?;

    let scripts = find_scripts(
        &finder_data.sprawl_packages,
        Path::new(&finder_data.sprawl_root),
    )
    .context("Failed to collect scripts")?;
    let script_path = scripts
        .get(&script.to_string())
        .with_context(|| {
            format_err!(
                "Couldn't find command {} in installed packages.\nInstalled scripts: {:?}",
                script,
                scripts.keys()
            )
        })?
        .to_path_buf();
    let scripts_tmp = TempDir::new().context("Failed to create tempdir")?;
    prepare_execve_environment(
        &scripts,
        &env::current_dir()?,
        scripts_tmp.path(),
        python_version,
    )?;

    let exit_code = if is_python_script(&script_path)? {
        debug!("launching (python) {}", script_path.display());
        let args: Vec<String> = [
            python_context.sys_executable.to_string_lossy().to_string(),
            script_path.to_string_lossy().to_string(),
        ]
        .into_iter()
        .chain(script_args)
        .collect();
        let exit_code = inject_and_run_python(
            &python_home,
            python_version,
            &args,
            &serde_json::to_string(&finder_data).unwrap(),
        )?;
        exit_code as i32
    } else {
        // Sorry for the to_string_lossy all over the place
        // https://stackoverflow.com/a/38948854/3549270
        let executable_c_str = CString::new(script_path.to_string_lossy().as_bytes())
            .context("Failed to convert executable path")?;
        let args_c_string = script_args
            .iter()
            .map(|arg| {
                CString::new(arg.as_bytes()).context("Failed to convert executable argument")
            })
            .collect::<anyhow::Result<Vec<CString>>>()?;

        debug!("launching (execv) {}", script_path.display());
        // We replace the current process with the new process is it's like actually just running
        // the real thing
        // note the that this may launch a python script, a native binary or anything else
        unistd::execv(&executable_c_str, &args_c_string).context("Failed to launch process")?;
        unreachable!()
    };
    // just to assert it lives until here
    drop(scripts_tmp);
    Ok(exit_code)
}
