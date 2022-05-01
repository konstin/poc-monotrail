use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::monotrail::setup_monotrail;
use anyhow::{bail, Context};
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyModule;
use pyo3::{pyfunction, pymodule, wrap_pyfunction, Py, PyAny, PyErr, PyResult, Python};
use std::env;
use std::path::{Path, PathBuf};
use tracing::debug;

static PEP508_QUERY_MODULE: &str = include_str!("get_pep508_env_function.py");

/// python has idiosyncratic cli options that are hard to replicate with clap, so we roll our own
///
/// `usage: python [option] ... [-c cmd | -m mod | file | -] [arg] ...`
fn naive_python_arg_parser<T: AsRef<str>>(args: &[T]) -> Result<Option<String>, String> {
    let bool_opts = [
        "-b", "-B", "-d", "-E", "-h", "-i", "-I", "-O", "-OO", "-q", "-s", "-S", "-u", "-v", "-V",
        "-x",
    ];
    let arg_opts = ["--check-hash-based-pycs", "-W", "-X"];
    let mut arg_iter = args.into_iter();
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

fn format_monotrail_error(err: anyhow::Error) -> PyErr {
    let mut accumulator = format!("{} failed to load.", env!("CARGO_PKG_NAME"));
    for cause in err.chain().collect::<Vec<_>>().iter() {
        accumulator.push_str(&format!("\n  Caused by: {}", cause));
    }
    PyRuntimeError::new_err(accumulator)
}

fn get_pep508_env(py: Python) -> PyResult<String> {
    let fun: Py<PyAny> = PyModule::from_code(
        py,
        PEP508_QUERY_MODULE,
        "get_pep508_env_direct.py",
        "get_pep508_env_direct",
    )?
    .getattr("get_pep508_env")?
    .into();

    // call object without empty arguments
    let json_string: String = fun.call0(py)?.extract(py)?;
    Ok(json_string)
}

/// Installs all required packages and returns package information to python
///
/// Parses the environment variables and returns the monotrail root and a list of
/// monotrail modules
#[pyfunction]
pub fn prepare_monotrail_from_env(
    py: Python,
    args: Vec<String>,
) -> PyResult<(String, Vec<InstalledPackage>)> {
    // We parse the python args even if we take MONOTRAIL_CWD as a validation
    // step
    let script = naive_python_arg_parser(&args).map_err(|err| PyRuntimeError::new_err(err))?;
    let script = if let Some(script) =
        env::var_os(&format!("{}_CWD", env!("CARGO_PKG_NAME").to_uppercase()))
    {
        Some(PathBuf::from(script))
    } else {
        script.map(PathBuf::from)
    };
    debug!("script: {:?}", script);
    let sys_executable: String = py.import("sys")?.getattr("executable")?.extract()?;
    let python_version = (py.version_info().major, py.version_info().minor);
    debug!("python: {:?} {}", python_version, sys_executable);
    let extras = parse_extras().map_err(format_monotrail_error)?;
    debug!("extras: {:?}", extras);
    let pep508_env = Pep508Environment::from_json_str(&get_pep508_env(py)?);

    setup_monotrail(
        script.as_deref(),
        Path::new(&sys_executable),
        python_version,
        &extras,
        &pep508_env,
    )
    .map_err(format_monotrail_error)
}

fn parse_extras() -> anyhow::Result<Vec<String>> {
    let extras_env_var = format!("{}_EXTRAS", env!("CARGO_PKG_NAME").to_uppercase());
    let extras = if let Some(extras) = env::var_os(&extras_env_var) {
        let extras: Vec<String> = extras
            .into_string()
            .ok() // can't use the original OsString
            .with_context(|| format!("{} must only contain utf-8 characters", extras_env_var))?
            .split(",")
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

/// Installs all required packages and returns package information to python
///
/// script can be a manually set working directory or the python script we're running.
/// Returns the monotrail root and a list of monotrail modules
#[pyfunction]
pub fn prepare_monotrail(
    py: Python,
    script: Option<String>,
    extras: Vec<String>,
    pep508_env: &str,
) -> PyResult<(String, Vec<InstalledPackage>)> {
    debug!("file for {}: {:?}", env!("CARGO_PKG_NAME"), script);
    let sys_executable: String = py.import("sys")?.getattr("executable")?.extract()?;

    setup_monotrail(
        script.as_deref().map(Path::new),
        Path::new(&sys_executable),
        (py.version_info().major, py.version_info().minor),
        &extras,
        &Pep508Environment::from_json_str(pep508_env),
    )
    .map_err(format_monotrail_error)
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
    m.add_function(wrap_pyfunction!(prepare_monotrail, m)?)?;
    m.add_function(wrap_pyfunction!(prepare_monotrail_from_env, m)?)?;
    m.add_class::<InstalledPackage>()?;
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
