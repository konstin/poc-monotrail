use crate::inject_and_run::{parse_plus_arg, run_python_args};
use crate::install::{filter_installed, install_all};
use crate::markers::marker_environment_from_python;
use crate::monotrail::{cli_from_git, monotrail_root, run_command};
use crate::package_index::download_distribution;
use crate::poetry_integration::read_dependencies::{read_poetry_specs, read_toml_files};
use crate::poetry_integration::run::poetry_run;
use crate::ppipx;
use crate::requirements_txt::RequirementsTxt;
use crate::spec::RequestedSpec;
use crate::utils::cache_dir;
use crate::venv_parser::get_venv_python_version;
use crate::verify_installation::verify_installation;
use anyhow::{bail, Context};
use clap::Parser;
use install_wheel_rs::{compatible_tags, Arch, InstallLocation, Os, WheelInstallerError};
use std::env;
use std::env::current_dir;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

#[derive(Parser, Debug)]
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
    /// Directory with the pyproject.toml, defaults to the current directory
    #[clap(long)]
    root: Option<PathBuf>,
    /// Only relevant for venv install
    #[clap(long)]
    skip_existing: bool,
    /// Don't bytecode compile python sources
    #[clap(long)]
    no_compile: bool,
}

/// Either `python ...` or `command ...`
#[derive(clap::Subcommand, Debug, Clone)]
pub enum RunSubcommand {
    /// Either `python ...` or `command ...`
    #[clap(external_subcommand)]
    Args(Vec<String>),
}

/// The main cli
#[derive(Parser, Debug)]
#[clap(version)]
pub enum Cli {
    /// Run with `python` or `command`. This features two subcommands that we unfortunately can't
    /// have as proper subcommands due to a clap bug
    /// (<https://github.com/clap-rs/clap/discussions/3766>)
    ///
    /// ### python
    ///
    /// Run a python file, module or script. Like `python <args...>`, but installs and injects the
    /// dependencies first.
    ///
    /// If you run python with a script, e.g. `python my/files/script.py`, monotrail will look for
    /// dependency specification (pyproject.toml or requirements.txt) next to script.py and up
    /// the file system tree.
    ///
    /// You can use the same arguments as for python main (they will be passed on), so you can do
    /// e.g. `monotrail run -p 3.9 python -OO -m http.server` instead of
    /// `python3.9 -OO -m http.server`.
    ///
    /// ### command
    ///
    /// Similar to the python command, but it starts an installed script such as e.g. `pytest` or
    /// `black`, not a .py file or a module
    Run {
        /// Install those extras from pyproject.toml
        #[clap(long, short = 'E')]
        extras: Vec<String>,
        /// Run this python version x.y. If you pass multiple versions it will run one after
        /// the other, just like tox
        #[clap(long, short)]
        python_version: Vec<String>,
        /// Directory with the pyproject.toml, defaults to the current directory
        #[clap(long)]
        root: Option<PathBuf>,
        /// Either `python ...` or `command ...`
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
        /// This contains first the command to run (e.g. `black` or `pytest`), which will also be
        /// used as package name unless --package is set, and then the arguments to be passed
        /// verbatim to the command. This is just `args` and not `command` and `args` due to
        /// limitations in clap (https://github.com/clap-rs/clap/discussions/3766)
        args: Vec<String>,
    },
    /// Like `git pull <repo> <tmpdir> && cd <tmpdir> && git checkout <rev> && monotrail run <...>`,
    /// mostly here to mirror the python `monotrail.from_git()` function
    FromGit {
        /// The git repository, e.g. `https://github.com/octocat/Spoon-Knife`
        git_url: String,
        /// The revision, e.g. `main` or `b0bab0`
        revision: String,
        /// extras to enable on the package e.g. `jupyter` for `black` to get `black[jupyter]`
        #[clap(long)]
        extras: Vec<String>,
        /// Run this python version x.y
        #[clap(long, short)]
        python_version: Option<String>,
        /// Either `python ...` or `command ...`
        #[clap(subcommand)]
        action: RunSubcommand,
    },
    /// Check all installed packages against their RECORD files
    VerifyInstallation {
        /// Print all offending paths
        #[clap(long, short)]
        verbose: bool,
    },
    /// Run the poetry bundled with monotrail. You can use the same command line options as with
    /// normally installed poetry, e.g. `monotrail poetry update` instead of `poetry update`
    #[clap(trailing_var_arg = true)]
    Poetry {
        /// arguments passed verbatim to poetry
        args: Vec<String>,
    },
    /// Install the given list of wheels in the current venv
    VenvInstall {
        /// The wheels to install
        targets: Vec<String>,
        /// skip pyc compilation
        #[clap(long)]
        no_compile: bool,
        /// run single threaded (mostly for profiling)
        #[clap(long)]
        no_parallel: bool,
    },
    /// Faster reimplementation of "poetry install" for both venvs and monotrail
    PoetryInstall {
        #[allow(missing_docs)]
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
    let target_dir = cache_dir()?.join("artifacts").join(name).join(version);
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

/// `poetry install` reimplementation that supports both venv and monotrail
fn poetry_install(
    venv: &Path,
    python_version: (u8, u8),
    venv_canon: &Path,
    options: &PoetryOptions,
) -> anyhow::Result<()> {
    let compatible_tags = compatible_tags(
        get_venv_python_version(venv)?,
        &Os::current()?,
        &Arch::current()?,
    )?;
    // TODO: don't parse this from a subprocess but do it like maturin
    let pep508_env = marker_environment_from_python(Path::new("python"));
    let dir = if let Some(root) = &options.root {
        root.clone()
    } else {
        env::current_dir()?
    };
    let (poetry_section, poetry_lock, _lockfile) =
        read_toml_files(&dir).context("Failed to read poetry files")?;
    let specs = read_poetry_specs(
        &poetry_section,
        poetry_lock,
        options.no_dev,
        &options.extras,
        &pep508_env,
    )
    .context("Failed to read poetry files")?;

    let location = if options.monotrail {
        let monotrail_root = monotrail_root()?;
        InstallLocation::Monotrail {
            monotrail_root,
            python: if cfg!(windows) {
                venv_canon.join("Scripts").join("python.exe")
            } else {
                venv_canon.join("bin").join("python")
            },
            python_version,
        }
    } else {
        let venv_base = venv
            .canonicalize()
            .context("Couldn't canonicalize venv location")?;
        InstallLocation::Venv {
            venv_base,
            python_version,
        }
    };

    let location = location.acquire_lock()?;
    let (to_install, mut installed_done) = if options.skip_existing || options.monotrail {
        filter_installed(&location, &specs, &compatible_tags)?
    } else {
        (specs, Vec::new())
    };
    let mut installed_new = install_all(
        &to_install,
        &location,
        &compatible_tags,
        options.no_compile,
        false,
        false,
    )?;
    installed_done.append(&mut installed_new);
    Ok(())
}

/// Dispatches from the Cli
///
/// The second parameter exists to override the venv in tests
pub fn run_cli(cli: Cli, venv: Option<&Path>) -> anyhow::Result<Option<i32>> {
    match cli {
        Cli::Run {
            extras,
            python_version,
            root,
            action,
        } => {
            let RunSubcommand::Args(args) = action;
            let trail_args = args[1..].to_vec();

            if python_version.len() <= 1 {
                let exit_code = match args[0].as_str() {
                    "python" => run_python_args(
                        &trail_args,
                        python_version.first().map(|x| x.as_str()),
                        root.as_deref(),
                        &extras,
                    )?,
                    "command" => run_command(
                        &extras,
                        python_version.first().map(|x| x.as_str()),
                        root.as_deref(),
                        // If there's no command this will show an error downstream
                        &args.get(1).unwrap_or(&"".to_string()),
                        &trail_args,
                    )?,
                    other => bail!("invalid command `{}`, must be 'python' or 'command'", other),
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
                        .args(&trail_args)
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
            args,
        } => Ok(Some(ppipx::ppipx(
            package.as_deref(),
            python_version.as_deref(),
            version.as_deref(),
            &extras,
            &args[0],
            &args,
        )?)),
        Cli::VerifyInstallation { verbose } => {
            let root = monotrail_root().context("Couldn't determine root")?;

            let paths = verify_installation(&root)?;
            if paths.is_empty() {
                println!("✔ All good. Packages verified in {}", root.display());
            } else {
                eprintln!("❌ Verification failed! Offending paths:");
                if verbose {
                    for path in paths {
                        eprintln!("{}", path)
                    }
                } else {
                    let max_paths = 10;
                    for path in paths.iter().take(max_paths) {
                        eprintln!("{}", path)
                    }
                    if paths.len() > max_paths {
                        eprintln!(
                            "... and {} more (use --verbose to see all)",
                            paths.len() - max_paths
                        );
                    }
                }
            }
            Ok(None)
        }
        Cli::Poetry { args } => Ok(Some(poetry_run(&args, None)?)),
        Cli::VenvInstall {
            targets,
            no_compile,
            no_parallel,
        } => {
            let venv = find_venv(venv)?;
            let python_version = get_venv_python_version(&venv)?;
            let venv_canon = venv.canonicalize()?;

            let compatible_tags = compatible_tags(
                get_venv_python_version(&venv)?,
                &Os::current()?,
                &Arch::current()?,
            )?;
            let location = InstallLocation::Venv {
                venv_base: venv_canon,
                python_version,
            }
            .acquire_lock()?;
            let specs = targets
                .iter()
                .map(|target| RequestedSpec::from_requested(target, &[]))
                .collect::<Result<Vec<RequestedSpec>, WheelInstallerError>>()?;

            install_all(
                &specs,
                &location,
                &compatible_tags,
                no_compile,
                false,
                no_parallel,
            )?;
            Ok(None)
        }
        Cli::PoetryInstall { options } => {
            let venv = find_venv(venv)?;
            let python_version = get_venv_python_version(&venv)?;
            let venv_canon = venv.canonicalize()?;
            poetry_install(&venv, python_version, &venv_canon, &options)
                .context("Failed to download and install")?;
            Ok(None)
        }
        Cli::FromGit {
            git_url,
            revision,
            extras,
            python_version,
            action,
        } => {
            let RunSubcommand::Args(args) = action;
            cli_from_git(&git_url, &revision, &extras, python_version, &args)
        }
    }
}

/// Finds a) an activated venv (`VIRTUAL_ENV`) b) `.venv` in any parent folder c) tells the user
/// about venvs
///
/// The optional argument allows overriding the venv in test
pub fn find_venv(venv: Option<&Path>) -> anyhow::Result<PathBuf> {
    let dot_venv = PathBuf::from(".venv");
    let venv = if let Some(venv) = venv {
        venv.to_path_buf()
    } else if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
        // According to https://peps.python.org/pep-0668/#marking-an-interpreter-as-using-an-external-package-manager
        // we should check `sys.prefix == sys.base_prefix`, but we're not a python program nor
        // running a python interpreter, in fact we want to create the environment
        PathBuf::from(virtual_env)
    } else if dot_venv.join("pyvenv.cfg").is_file() {
        dot_venv
    } else {
        let activation_command = if cfg!(target_family = "unix") {
            "and activate it using `source .venv/bin/activate`".to_string()
        } else if cfg!(target_family = "windows") {
            r#"and activate it using `.venv\Scripts\Activate.ps1`"#.to_string()
        } else {
            format!(
                ". Please consult the documentation for {} on how to activate virtualenvs. ",
                target_lexicon::HOST
            )
        };
        bail!(
            "Couldn't find an activated virtualenv not a .venv found in any parent directory. \
                    You can create a virtualenv with `python -m venv .venv` {}. \
                    See https://virtualenv.pypa.io/en/latest/index.html for more information",
            activation_command
        );
    };
    Ok(venv)
}
