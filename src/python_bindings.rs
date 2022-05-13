//! I'm trying to keep the interface simple and reusable for the rust main binary case
//! by shipping all information through `FinderData`.
//!
//! TODO: Be consistent with String vs. PathBuf

use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::monotrail::{get_specs, install_requested, spec_paths};
use crate::poetry_integration::lock::poetry_resolve;
use crate::poetry_integration::read_dependencies::specs_from_git;
use crate::spec::RequestedSpec;
use crate::{read_poetry_specs, PEP508_QUERY_ENV};
use anyhow::{bail, Context};
use install_wheel_rs::{Arch, Os};
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyModule;
use pyo3::{pyclass, pyfunction, pymodule, wrap_pyfunction, Py, PyAny, PyErr, PyResult, Python};
use std::collections::HashMap;
use std::env;
use std::option::Option::None;
use std::path::{Path, PathBuf};
use tracing::{debug, trace};

/// The packaging and import data that is resolved by the rust part and deployed by the finder
#[pyclass]
pub struct FinderData {
    /// The location where all packages are installed
    #[pyo3(get)]
    sprawl_root: String,
    /// All resolved and installed packages indexed by name
    #[pyo3(get)]
    sprawl_packages: Vec<InstalledPackage>,
    /// Given a module name, where's the corresponding module file and what are the submodule_search_locations?
    #[pyo3(get)]
    spec_paths: HashMap<String, (PathBuf, Vec<PathBuf>)>,
    /// In from git mode where we check out a repository and make it available for import as if it was added to sys.path
    #[pyo3(get)]
    repo_dir: Option<PathBuf>,
    /// We need to run .pth files because some project such as matplotlib 3.5.1 use them to commit packaging crimes
    #[pyo3(get)]
    pth_files: Vec<PathBuf>,
    /// The contents of the last poetry.lock, used a basis for the next resolution when requirements
    /// change at runtime, both for faster resolution and in hopes the exact version stay the same
    /// so the user doesn't need to reload python
    #[pyo3(get)]
    lockfile: String,
    /// The installed scripts indexed by name. They are in the bin folder of each project, coming
    /// from entry_points.txt or data folder scripts
    #[pyo3(get)]
    scripts: HashMap<String, String>,
}

/// python has idiosyncratic cli options that are hard to replicate with clap, so we roll our own
///
/// `usage: python [option] ... [-c cmd | -m mod | file | -] [arg] ...`
fn naive_python_arg_parser<T: AsRef<str>>(args: &[T]) -> Result<Option<String>, String> {
    let bool_opts = [
        "-b", "-B", "-d", "-E", "-h", "-i", "-I", "-O", "-OO", "-q", "-s", "-S", "-u", "-v", "-V",
        "-x",
    ];
    let arg_opts = ["--check-hash-based-pycs", "-W", "-X"];
    let mut arg_iter = args.iter();
    loop {
        if let Some(arg) = arg_iter.next() {
            if bool_opts.contains(&arg.as_ref()) {
                continue;
            } else if arg_opts.contains(&arg.as_ref()) {
                let value = arg_iter.next();
                if value.is_none() {
                    return Err(format!("Missing argument for {}", arg.as_ref()));
                }
                continue;
            } else if arg.as_ref() == "-c" || arg.as_ref() == "-m" {
                let value = arg_iter.next();
                if value.is_none() {
                    return Err(format!("Missing argument for {}", arg.as_ref()));
                }
                return Ok(None);
            } else {
                return Ok(Some(arg.as_ref().to_string()));
            }
        } else {
            // interactive python shell
            return Ok(None);
        }
    }
}

fn format_monotrail_error(err: impl Into<anyhow::Error>) -> PyErr {
    let mut accumulator = format!("{} failed to load.", env!("CARGO_PKG_NAME"));
    for cause in err.into().chain().collect::<Vec<_>>().iter() {
        accumulator.push_str(&format!("\n  Caused by: {}", cause));
    }
    PyRuntimeError::new_err(accumulator)
}

/// Uses the python C API to run a code snippet that json encodes the PEP508 env
fn get_pep508_env(py: Python) -> PyResult<String> {
    let fun: Py<PyAny> =
        PyModule::from_code(py, PEP508_QUERY_ENV, "get_pep508_env.py", "get_pep508_env")?
            .getattr("get_pep508_env")?
            .into();

    // call object without empty arguments
    let json_string: String = fun.call0(py)?.extract(py)?;
    Ok(json_string)
}

fn install_specs_to_finder(
    specs: &[RequestedSpec],
    sys_executable: String,
    python_version: (u8, u8),
    scripts: HashMap<String, String>,
    lockfile: String,
    repo_dir: Option<PathBuf>,
) -> PyResult<FinderData> {
    let (sprawl_root, sprawl_packages) =
        install_requested(&specs, sys_executable.as_ref(), python_version)
            .map_err(format_monotrail_error)?;
    let (spec_paths, pth_files) =
        spec_paths(sprawl_root.as_ref(), &sprawl_packages, python_version)
            .map_err(format_monotrail_error)?;

    Ok(FinderData {
        sprawl_root,
        sprawl_packages,
        spec_paths,
        repo_dir,
        pth_files,
        lockfile,
        scripts,
    })
}

fn get_python_platform(py: Python) -> PyResult<(String, (u8, u8), Os, Arch)> {
    let sys_executable: String = py.import("sys")?.getattr("executable")?.extract()?;
    let python_version = (py.version_info().major, py.version_info().minor);
    let os = Os::current().map_err(format_monotrail_error)?;
    let arch = Arch::current().map_err(format_monotrail_error)?;
    debug!(
        "python: {:?} {:?} {} {}",
        python_version, os, arch, sys_executable
    );
    Ok((sys_executable, python_version, os, arch))
}

/// Installs all required packages and returns package information to python, while parsing
/// the setup from the environment variables (or defaults)
#[pyfunction]
pub fn monotrail_from_env(py: Python, args: Vec<String>) -> PyResult<FinderData> {
    // We parse the python args even if we take MONOTRAIL_CWD as a validation
    // step
    let script = naive_python_arg_parser(&args).map_err(PyRuntimeError::new_err)?;
    let script = if let Some(script) =
        env::var_os(&format!("{}_CWD", env!("CARGO_PKG_NAME").to_uppercase()))
    {
        Some(PathBuf::from(script))
    } else {
        script.map(PathBuf::from)
    };
    debug!("monotrail_from_env script: {:?}", script);
    let (sys_executable, python_version, _, _) = get_python_platform(py)?;
    let extras = parse_extras().map_err(format_monotrail_error)?;
    debug!("extras: {:?}", extras);
    let pep508_env = Pep508Environment::from_json_str(&get_pep508_env(py)?);

    let (specs, scripts, lockfile) = get_specs(
        script.as_deref(),
        &extras,
        Path::new(&sys_executable),
        python_version,
        &pep508_env,
    )
    .map_err(format_monotrail_error)?;

    install_specs_to_finder(
        &specs,
        sys_executable,
        python_version,
        scripts,
        lockfile,
        None,
    )
}

/// User gives a `[tool.poetry.dependencies]`
#[pyfunction]
pub fn monotrail_from_requested(
    py: Python,
    requested: String,
    lockfile: Option<String>,
) -> PyResult<FinderData> {
    let requested = serde_json::from_str(&requested)
        .map_err(|serde_err| PyRuntimeError::new_err(format!("Invalid dependency format: {}.\n See https://python-poetry.org/docs/dependency-specification/", serde_err)))?;

    let (sys_executable, python_version, _, _) = get_python_platform(py)?;
    let pep508_env = Pep508Environment::from_json_str(&get_pep508_env(py)?);

    let (poetry_toml, poetry_lock, lockfile) = poetry_resolve(
        requested,
        Path::new(&sys_executable),
        python_version,
        lockfile.as_deref(),
        &pep508_env,
    )
    .context("Failed to resolve requested dependencies through poetry")
    .map_err(format_monotrail_error)?;
    let specs = read_poetry_specs(poetry_toml, poetry_lock, false, &[], &pep508_env)
        .map_err(format_monotrail_error)?;

    install_specs_to_finder(
        &specs,
        sys_executable,
        python_version,
        HashMap::new(),
        lockfile,
        None,
    )
}

/// Checkouts the repository at the given revision, storing it in the user cache dir.
#[pyfunction]
pub fn monotrail_from_git(
    py: Python,
    git_url: String,
    revision: String,
    extras: Option<Vec<String>>,
    lockfile: Option<String>,
) -> PyResult<FinderData> {
    debug!("monotrail_from_git: {} {}", git_url, revision);
    let (sys_executable, python_version, _, _) = get_python_platform(py)?;
    debug!("extras: {:?}", extras);
    let pep508_env = Pep508Environment::from_json_str(&get_pep508_env(py)?);

    let (specs, repo_dir, lockfile) = specs_from_git(
        git_url,
        revision,
        extras.as_deref().unwrap_or_default(),
        lockfile.as_deref(),
        sys_executable.as_ref(),
        python_version,
        &pep508_env,
    )
    .map_err(format_monotrail_error)?;

    install_specs_to_finder(
        &specs,
        sys_executable,
        python_version,
        HashMap::new(),
        lockfile,
        Some(repo_dir),
    )
}

/// Like monotrail_from_env, except you explicitly pass what you want, currently only used for
/// testing
#[pyfunction]
pub fn monotrail_from_dir(py: Python, dir: PathBuf, extras: Vec<String>) -> PyResult<FinderData> {
    debug!("monotrail_from_dir script: {:?}", dir);
    let (sys_executable, python_version, _, _) = get_python_platform(py)?;
    debug!("extras: {:?}", extras);
    let pep508_env = Pep508Environment::from_json_str(&get_pep508_env(py)?);

    let (specs, scripts, lockfile) = get_specs(
        Some(&dir),
        &extras,
        Path::new(&sys_executable),
        python_version,
        &pep508_env,
    )
    .map_err(format_monotrail_error)?;

    install_specs_to_finder(
        &specs,
        sys_executable,
        python_version,
        scripts,
        lockfile,
        None,
    )
}

/// The installed packages are all lies and rumors, we can only find the actually importable
/// packages by walking the site-packages, so here we map installed packages to importable modules
/// and a list of .pth we need to run
#[pyfunction]
pub fn monotrail_spec_paths(
    py: Python,
    sprawl_root: PathBuf,
    sprawl_packages: Vec<InstalledPackage>,
) -> PyResult<(HashMap<String, (PathBuf, Vec<PathBuf>)>, Vec<PathBuf>)> {
    let python_version = (py.version_info().major, py.version_info().minor);
    let (modules, pth_files) = spec_paths(&sprawl_root, &sprawl_packages, python_version)
        .map_err(format_monotrail_error)?;
    trace!(
        "Available modules: {}",
        modules.keys().map(|s| &**s).collect::<Vec<_>>().join(" ")
    );
    Ok((modules, pth_files))
}

fn parse_extras() -> anyhow::Result<Vec<String>> {
    let extras_env_var = format!("{}_EXTRAS", env!("CARGO_PKG_NAME").to_uppercase());
    let extras = if let Some(extras) = env::var_os(&extras_env_var) {
        let extras: Vec<String> = extras
            .into_string()
            .ok() // can't use the original OsString
            .with_context(|| format!("{} must only contain utf-8 characters", extras_env_var))?
            .split(',')
            .map(ToString::to_string)
            .collect();
        for extra in &extras {
            let allowed = |x: char| x.is_alphanumeric() || x == '-' || x == '_';
            if !extra.chars().all(allowed) {
                bail!(
                    "Invalid extra name '{}', allowed are underscore, minus, letters and digits",
                    extra
                );
            }
        }
        extras
    } else {
        Vec::new()
    };
    Ok(extras)
}

#[pymodule]
pub fn monotrail(_py: Python, m: &PyModule) -> PyResult<()> {
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
    m.add_function(wrap_pyfunction!(monotrail_from_env, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_from_requested, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_from_git, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_spec_paths, m)?)?;
    m.add("project_name", env!("CARGO_PKG_NAME"))?;
    m.add_class::<InstalledPackage>()?;
    m.add_class::<FinderData>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::naive_python_arg_parser;

    #[test]
    fn test_naive_python_arg_parser() {
        let cases: &[(&[&str], _)] = &[
            (
                &["-v", "-m", "mymod", "--first_arg", "second_arg"],
                Ok(None),
            ),
            (
                &["-v", "my_script.py", "--first_arg", "second_arg"],
                Ok(Some("my_script.py".to_string())),
            ),
            (&["-v"], Ok(None)),
            (&[], Ok(None)),
            (&["-m"], Err("Missing argument for -m".to_string())),
        ];
        for (args, parsing) in cases {
            assert_eq!(&naive_python_arg_parser(args), parsing);
        }
    }
}
