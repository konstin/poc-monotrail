use crate::{install_wheel, InstallLocation, WheelInstallerError};
use pyo3::create_exception;
use pyo3::types::PyModule;
use pyo3::{pyfunction, pymodule, wrap_pyfunction, PyErr, PyResult, Python};
use std::env;
use std::error::Error;
use std::path::PathBuf;

create_exception!(
    install_wheel_rs,
    PyWheelInstallerError,
    pyo3::exceptions::PyException
);

impl From<WheelInstallerError> for PyErr {
    fn from(err: WheelInstallerError) -> Self {
        let mut accumulator = format!("Failed to install wheels: {}", err);

        let mut current_err: &dyn Error = &err;
        while let Some(cause) = current_err.source() {
            accumulator.push_str(&format!("\n  Caused by: {}", cause));
            current_err = cause;
        }
        PyWheelInstallerError::new_err(accumulator)
    }
}

#[pyfunction]
pub fn install_wheels_venv(py: Python, wheels: Vec<PathBuf>, venv: PathBuf) -> PyResult<()> {
    let python_version = (py.version_info().major, py.version_info().minor);

    // TODO: parallelize, and if so, how do we handle conflict?
    let location = InstallLocation::Venv {
        venv_base: venv,
        python_version,
    }
    .acquire_lock()
    .map_err(WheelInstallerError::from)?;
    for wheel in wheels {
        // TODO: Pass those options on to the user
        // unique_version can be anything since it's only used to monotrail
        py.allow_threads(|| install_wheel(&location, &wheel, true, &[], ""))?;
    }
    Ok(())
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
    m.add_function(wrap_pyfunction!(install_wheels_venv, m)?)?;
    Ok(())
}
