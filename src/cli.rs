use crate::inject_and_run::{
    determine_python_version, inject_and_run_python, parse_major_minor, parse_plus_arg,
    prepare_execve_environment, run_python_args,
};
use crate::install::{filter_installed, InstalledPackage};
use crate::markers::Pep508Environment;
use crate::monotrail::{
    find_scripts, install_specs_to_finder, is_python_script, monotrail_root, FinderData,
    PythonContext,
};
use crate::package_index::download_distribution;
use crate::poetry_integration::lock::poetry_resolve_from_dir;
use crate::poetry_integration::poetry_toml;
use crate::poetry_integration::poetry_toml::PoetryPyprojectToml;
use crate::poetry_integration::read_dependencies::{read_poetry_specs, read_toml_files};
use crate::poetry_integration::run::poetry_run;
use crate::spec::RequestedSpec;
use crate::standalone_python::provision_python;
use crate::venv_parser::get_venv_python_version;
use crate::{data_local_dir, get_specs, install_specs, DEFAULT_PYTHON_VERSION};
use anyhow::{bail, format_err, Context};
use clap::Parser;
use fs_err as fs;
use install_wheel_rs::{compatible_tags, Arch, InstallLocation, Os, WheelInstallerError};
use nix::unistd;
use std::collections::BTreeMap;
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tracing::{debug, info};

#[derive(Parser)]
pub struct PoetryOptions {
    /// Don't install dev dependencies
    #[clap(long)]
    no_dev: bool,
    /// The extras for which the dependencies should be installed
    #[clap(long, short = 'E')]
    extras: Vec<String>,
    /// Whether to install in a venv or the monotrail cache
    #[clap(long)]
    monotrail: bool,
    /// Only relevant for venv install
    #[clap(long)]
    skip_existing: bool,
    /// Don't bytecode compile python sources
    #[clap(long)]
    no_compile: bool,
}

#[derive(clap::Subcommand)]
pub enum RunSubcommand {
    /// Like `python <args...>`, but installs and injects the dependencies before.
    ///
    /// If you run python with a script, e.g. `python my/files/script.py`, monotrail will look for
    /// dependency specification (pyproject.toml or requirements.txt) next to script.py and up
    /// the file system tree.
    ///
    /// You can use the same arguments as for python main (they will be passed on), so you can do
    /// e.g. `monotrail run -p 3.9 python -OO -m http.server` instead of
    /// `python3.9 -OO -m http.server`.
    #[clap(trailing_var_arg = true)]
    Python { args: Vec<String> },
    /// Similar to the python command, but it starts an installed script such as e.g. `pytest` or
    /// `black`, not a .py file or a module
    #[clap(trailing_var_arg = true)]
    Command { command: String, args: Vec<String> },
}

#[derive(Parser)]
pub enum Cli {
    /// Run a python file, module or script. Works like `python <args...>`, but installs and injects
    /// the dependencies before.
    Run {
        /// Install those extras from pyproject.toml
        #[clap(long, short = 'E')]
        extras: Vec<String>,
        /// Run this python version x.y. If you pass multiple versions it will run one after
        /// the other, just like tox
        #[clap(long, short)]
        python_version: Vec<String>,
        /// Directory with the pyproject.toml
        #[clap(long)]
        root: Option<PathBuf>,
        #[clap(subcommand)]
        action: RunSubcommand,
    },
    /// pseudo-pipx. Runs a command from a package
    #[clap(trailing_var_arg = true)]
    Ppipx {
        /// name of the pypi package that contains the command
        #[clap(long)]
        package: Option<String>,
        /// Run this python version x.y
        #[clap(long, short)]
        python_version: Option<String>,
        /// version to pass to poetry
        #[clap(long)]
        version: Option<String>,
        /// extras to enable on the package e.g. `jupyter` for `black` to get `black[jupyter]`
        #[clap(long)]
        extras: Vec<String>,
        /// command to run (e.g. `black` or `pytest`), will also be used as package name unless
        /// --package is set
        command: String,
        args: Vec<String>,
    },
    /// Run the poetry bundled with monotrail. You can use the same command line options as with
    /// normally installed poetry, e.g. `monotrail poetry update` instead of `poetry update`
    #[clap(trailing_var_arg = true)]
    Poetry { args: Vec<String> },
    /// Install the given list of wheels in the current venv
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
}

/// Builds cache filename, downloads if not present, returns cache filename
pub fn download_distribution_cached(
    name: &str,
    version: &str,
    filename: &str,
    url: &str,
) -> anyhow::Result<PathBuf> {
    let target_dir = crate::cache_dir()?
        .join("artifacts")
        .join(name)
        .join(version);
    let target_file = target_dir.join(&filename);

    if target_file.is_file() {
        debug!(
            "Found {} {} cached at {}",
            name,
            version,
            target_file.display()
        );
        return Ok(target_file);
    }

    // TODO: Lookup size and show it somewhere if it's large
    debug!("Downloading {} {}", name, version);
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
        } => {
            let args = match &action {
                RunSubcommand::Python { args } => args,
                RunSubcommand::Command { args, .. } => args,
            };
            if python_version.len() <= 1 {
                let exit_code = match &action {
                    RunSubcommand::Python { .. } => run_python_args(
                        &args,
                        python_version.first().map(|x| x.as_str()),
                        root.as_deref(),
                        &extras,
                    )?,
                    RunSubcommand::Command {
                        command: script, ..
                    } => run_script(
                        &extras,
                        python_version.first().map(|x| x.as_str()),
                        &script,
                        &args,
                    )?,
                };
                Ok(Some(exit_code))
            } else {
                if parse_plus_arg(&args)?.1.is_some() {
                    bail!("You can't use a +x.y version when specifying multiple --python-version")
                }

                for version in python_version {
                    info!("Running {}", version);
                    // To avoid running into TLS and such issues (e.g. sys.path is broken if we
                    // don't), we spawn a new process for each python version. Could be easily
                    // extended to run this in parallel.
                    // Would be nicer to use a fork wrapper here
                    let status = Command::new(env::current_exe()?)
                        .args(&["run", "-p", &version, "python"])
                        .args(args)
                        .status()
                        .context("Failed to start child process for python version")?;
                    if !status.success() {
                        bail!("Python exited with {:?}", status);
                    }
                }
                Ok(None)
            }
        }
        Cli::Ppipx {
            package,
            python_version,
            version,
            extras,
            command,
            args,
        } => Ok(Some(ppipx(
            package.as_deref(),
            python_version.as_deref(),
            version.as_deref(),
            &extras,
            &command,
            &args,
        )?)),
        Cli::Poetry { args } => Ok(Some(poetry_run(&args, None)?)),
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
    }
}

fn run_script(
    extras: &[String],
    python_version: Option<&str>,
    script: &str,
    args: &[String],
) -> anyhow::Result<i32> {
    let (args, python_version) = determine_python_version(args, python_version)?;
    let (python_context, python_home) = provision_python(python_version)?;

    let (specs, wrong_scripts, lockfile) = get_specs(None, extras, &python_context)?;
    let finder_data =
        install_specs_to_finder(&specs, wrong_scripts, lockfile, None, &python_context)?;

    let exit_code = run_command_finder_data(
        &script,
        &args,
        &python_context,
        &python_home,
        &env::current_dir()?,
        &finder_data,
    )?;
    Ok(exit_code)
}

/// Simple pipx reimplementation
///
/// Resolves one package, saving it in .local and runs one command from it
fn ppipx(
    package: Option<&str>,
    python_version: Option<&str>,
    version: Option<&str>,
    extras: &[String],
    command: &str,
    args: &[String],
) -> anyhow::Result<i32> {
    let python_version = python_version
        .map(parse_major_minor)
        .transpose()?
        .unwrap_or(DEFAULT_PYTHON_VERSION);

    let (python_context, python_home) = provision_python(python_version)?;
    let package = package.unwrap_or(command);
    let package_extras = if extras.is_empty() {
        package.to_string()
    } else {
        format!("{}[{}]", package, extras.join(","))
    };

    let resolution_dir = data_local_dir()?
        .join("ppipx")
        .join(&package_extras)
        .join(version.unwrap_or("latest"));

    if !resolution_dir.join("poetry.lock").is_file() {
        info!(
            "Generating ppipx entry for {}@{}",
            package_extras,
            version.unwrap_or("latest")
        );
        let mut dependencies = BTreeMap::new();
        // Add python entry with current version; resolving will otherwise fail with complaints
        dependencies.insert(
            "python".to_string(),
            // For some reason on github actions 3.8.12 is not 3.8 compatible, so we name the range explicitly
            poetry_toml::Dependency::Compact(format!(
                ">={}.{},<{}.{}",
                python_version.0,
                python_version.1,
                python_version.0,
                python_version.1 + 1
            )),
        );
        if extras.is_empty() {
            dependencies.insert(
                package.to_string(),
                poetry_toml::Dependency::Compact(version.unwrap_or("*").to_string()),
            );
        } else {
            dependencies.insert(
                package.to_string(),
                poetry_toml::Dependency::Expanded {
                    version: Some(version.unwrap_or("*").to_string()),
                    optional: None,
                    extras: Some(extras.to_vec()),
                    git: None,
                    branch: None,
                },
            );
        }
        let pyproject_toml = PoetryPyprojectToml {
            tool: Some(poetry_toml::ToolSection {
                poetry: Some(poetry_toml::PoetrySection {
                    name: format!("{}_launcher", package),
                    version: "0.0.1".to_string(),
                    description: format!(
                        "Launcher for {}@{}",
                        package,
                        version.unwrap_or("latest")
                    ),
                    authors: vec!["monotrail".to_string()],
                    dependencies,
                    dev_dependencies: Default::default(),
                    extras: None,
                    scripts: None,
                }),
            }),
            build_system: None,
        };

        fs::create_dir_all(&resolution_dir).context("Failed to create ppipx resolution dir")?;
        let resolve_dir = TempDir::new()?;
        fs::write(
            resolve_dir.path().join("pyproject.toml"),
            toml::to_vec(&pyproject_toml)
                .context("Failed to serialize pyproject.toml for ppipx")?,
        )?;
        poetry_resolve_from_dir(&resolve_dir, &python_context)?;
        fs::copy(
            resolve_dir.path().join("pyproject.toml"),
            resolution_dir.join("pyproject.toml"),
        )
        .context("Failed to copy ppipx pyproject.toml")?;
        fs::copy(
            resolve_dir.path().join("poetry.lock"),
            resolution_dir.join("poetry.lock"),
        )
        .context("Poetry didn't generate a poetry.lock")?;
    } else {
        debug!("ppipx entry already present")
    }

    let (poetry_section, poetry_lock, lockfile) = read_toml_files(&resolution_dir)
        .with_context(|| format!("Invalid ppipx entry at {}", resolution_dir.display()))?;
    let specs = read_poetry_specs(
        &poetry_section,
        poetry_lock,
        true,
        &[],
        &python_context.pep508_env,
    )?;

    let finder_data =
        install_specs_to_finder(&specs, BTreeMap::new(), lockfile, None, &python_context)
            .context("Couldn't install packages")?;

    run_command_finder_data(
        &command,
        &args,
        &python_context,
        &python_home,
        &resolution_dir,
        &finder_data,
    )
}

fn run_command_finder_data(
    script: &str,
    args: &[String],
    python_context: &PythonContext,
    python_home: &Path,
    root: &Path,
    finder_data: &FinderData,
) -> anyhow::Result<i32> {
    let scripts = find_scripts(
        &finder_data.sprawl_packages,
        Path::new(&finder_data.sprawl_root),
    )
    .context("Failed to collect scripts")?;
    let scripts_tmp = TempDir::new().context("Failed to create tempdir")?;
    prepare_execve_environment(&scripts, &root, scripts_tmp.path(), python_context.version)?;

    let script_path = scripts.get(&script.to_string()).with_context(|| {
        format_err!(
            "Couldn't find command {} in installed packages.\nInstalled scripts: {:?}",
            script,
            scripts.keys()
        )
    })?;
    let exit_code = if is_python_script(&script_path)? {
        debug!("launching (python) {}", script_path.display());
        let args: Vec<String> = [
            python_context.sys_executable.to_string_lossy().to_string(),
            script_path.to_string_lossy().to_string(),
        ]
        .iter()
        .chain(args)
        .map(ToString::to_string)
        .collect();
        let exit_code = inject_and_run_python(
            &python_home,
            python_context.version,
            &args,
            &serde_json::to_string(&finder_data).unwrap(),
        )?;
        exit_code as i32
    } else {
        // Sorry for the to_string_lossy all over the place
        // https://stackoverflow.com/a/38948854/3549270
        let executable_c_str = CString::new(script_path.to_string_lossy().as_bytes())
            .context("Failed to convert executable path")?;
        let args_c_string = args
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
