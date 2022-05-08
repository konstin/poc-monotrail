use crate::{install_wheel, InstallLocation, LockedDir, WheelInstallerError};
use pyo3::create_exception;
use pyo3::types::PyModule;
use pyo3::{pyclass, pymethods, pymodule, PyErr, PyResult, Python};
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

#[pyclass]
struct LockedVenv {
    location: InstallLocation<LockedDir>,
}

#[pymethods]
impl LockedVenv {
    #[new]
    pub fn new(py: Python, venv: PathBuf) -> PyResult<Self> {
        Ok(Self {
            location: InstallLocation::Venv {
                venv_base: LockedDir::acquire(&venv)?,
                python_version: (py.version_info().major, py.version_info().minor),
            },
        })
    }

    pub fn install_wheel(&self, py: Python, wheel: PathBuf) -> PyResult<()> {
        // TODO: Pass those options on to the user
        // unique_version can be anything since it's only used to monotrail
        py.allow_threads(|| install_wheel(&self.location, &wheel, true, &[], ""))?;
        Ok(())
    }
}

#[pymodule]
pub fn install_wheel_rs(_py: Python, m: &PyModule) -> PyResult<()> {
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
    m.add_class::<LockedVenv>()?;
    Ok(())
}
