use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::monorail::setup_monorail;
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyModule;
use pyo3::{pyfunction, pymodule, wrap_pyfunction, PyResult, Python};
use std::env;
use std::path::Path;
use tracing::debug;

/// Installs all required packages and returns package information to python
/// Takes the python script that is run and returns the monorail root and a list of
/// monorail modules (name, python_version, unique_version)
#[pyfunction]
fn prepare_monorail(
    py: Python,
    file_running: Option<String>,
    extras: Vec<String>,
    pep508_env: &str,
) -> PyResult<(String, Vec<InstalledPackage>)> {
    debug!("file for {}: {:?}", env!("CARGO_PKG_NAME"), file_running);
    let sys_executable: String = py.import("sys")?.getattr("executable")?.extract()?;

    let result = setup_monorail(
        file_running.as_deref().map(Path::new),
        Path::new(&sys_executable),
        (py.version_info().major, py.version_info().minor),
        &extras,
        &Pep508Environment::from_json_str(pep508_env),
    );
    result.map_err(|err| {
        let mut accumulator = format!("{} failed to load.", env!("CARGO_PKG_NAME"));
        for cause in err.chain().collect::<Vec<_>>().iter() {
            accumulator.push_str(&format!("\n  Caused by: {}", cause));
        }
        PyRuntimeError::new_err(accumulator)
    })
}

#[pymodule]
fn monorail(_py: Python, m: &PyModule) -> PyResult<()> {
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
    m.add_function(wrap_pyfunction!(prepare_monorail, m)?)?;
    m.add_class::<InstalledPackage>()?;
    Ok(())
}
