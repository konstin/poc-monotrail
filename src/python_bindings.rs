//! I'm trying to keep the interface simple and reusable for the rust main binary case
//! by shipping all information through `FinderData`.
//!
//! TODO: Be consistent with String vs. PathBuf

use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::monotrail::{
    get_specs, install_specs_to_finder, spec_paths, FinderData, LaunchType, PythonContext,
};
use crate::poetry_integration::lock::poetry_resolve;
use crate::poetry_integration::read_dependencies::specs_from_git;
use crate::{inject_and_run, read_poetry_specs, PEP508_QUERY_ENV};
use anyhow::{bail, Context};
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyModule;
use pyo3::{pyfunction, pymodule, wrap_pyfunction, Py, PyAny, PyErr, PyResult, Python};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use tracing::{debug, trace};

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

fn get_python_context(py: Python) -> PyResult<PythonContext> {
    // Would be nicer through https://docs.python.org/3/c-api/init.html#c.Py_GetProgramFullPath
    let sys_executable: String = py.import("sys")?.getattr("executable")?.extract()?;
    let python_context = PythonContext {
        sys_executable: PathBuf::from(sys_executable),
        python_version: (py.version_info().major, py.version_info().minor),
        pep508_env: Pep508Environment::from_json_str(&get_pep508_env(py)?),
        launch_type: LaunchType::PythonBindings,
    };
    debug!("python: {:?}", python_context);
    Ok(python_context)
}

/// Takes a python invocation, extracts the script dir (if any), installs all required packages
/// and returns script dir and finder data to python
#[pyfunction]
pub fn monotrail_from_args(py: Python, args: Vec<String>) -> PyResult<FinderData> {
    // We parse the python args even if we take MONOTRAIL_CWD as a validation
    // step
    let script = inject_and_run::naive_python_arg_parser(&args).map_err(PyRuntimeError::new_err)?;
    let script = if let Some(script) =
        env::var_os(&format!("{}_CWD", env!("CARGO_PKG_NAME").to_uppercase()))
    {
        Some(PathBuf::from(script))
    } else {
        script.map(PathBuf::from)
    };
    debug!("monotrail_from_args script: {:?}, args: {:?}", script, args);
    let python_context = get_python_context(py)?;
    let extras = parse_extras().map_err(format_monotrail_error)?;
    debug!("extras: {:?}", extras);

    let (specs, scripts, lockfile) =
        get_specs(script.as_deref(), &extras, &python_context).map_err(format_monotrail_error)?;
    install_specs_to_finder(&specs, scripts, lockfile, None, &python_context)
        .map_err(format_monotrail_error)
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

    let python_context = get_python_context(py)?;
    let pep508_env = Pep508Environment::from_json_str(&get_pep508_env(py)?);

    let (poetry_toml, poetry_lock, lockfile) =
        poetry_resolve(requested, lockfile.as_deref(), &python_context)
            .context("Failed to resolve requested dependencies through poetry")
            .map_err(format_monotrail_error)?;
    let specs = read_poetry_specs(poetry_toml, poetry_lock, false, &[], &pep508_env)
        .map_err(format_monotrail_error)?;

    install_specs_to_finder(&specs, HashMap::new(), lockfile, None, &python_context)
        .map_err(format_monotrail_error)
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
    let python_context = get_python_context(py)?;
    debug!("extras: {:?}", extras);

    let (specs, repo_dir, lockfile) = specs_from_git(
        git_url,
        revision,
        extras.as_deref().unwrap_or_default(),
        lockfile.as_deref(),
        &python_context,
    )
    .map_err(format_monotrail_error)?;

    install_specs_to_finder(
        &specs,
        HashMap::new(),
        lockfile,
        Some(repo_dir),
        &python_context,
    )
    .map_err(format_monotrail_error)
}

/// Like monotrail_from_args, except you explicitly pass what you want, currently only used for
/// testing
#[pyfunction]
pub fn monotrail_from_dir(py: Python, dir: PathBuf, extras: Vec<String>) -> PyResult<FinderData> {
    debug!("monotrail_from_dir script: {:?}", dir);
    let python_context = get_python_context(py)?;
    debug!("extras: {:?}", extras);

    let (specs, scripts, lockfile) =
        get_specs(Some(&dir), &extras, &python_context).map_err(format_monotrail_error)?;

    install_specs_to_finder(&specs, scripts, lockfile, None, &python_context)
        .map_err(format_monotrail_error)
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
    m.add_function(wrap_pyfunction!(monotrail_from_args, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_from_requested, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_from_dir, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_from_git, m)?)?;
    m.add_function(wrap_pyfunction!(monotrail_spec_paths, m)?)?;
    m.add("project_name", env!("CARGO_PKG_NAME"))?;
    m.add_class::<InstalledPackage>()?;
    m.add_class::<FinderData>()?;
    Ok(())
}
