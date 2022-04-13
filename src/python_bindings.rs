use crate::markers::Pep508Environment;
use crate::virtual_sprawl::setup_virtual_sprawl;
use pyo3::exceptions::PyRuntimeError;
use pyo3::types::PyModule;
use pyo3::{pyfunction, pymodule, wrap_pyfunction, PyResult, Python};
use std::env;
use std::path::Path;
use tracing::debug;

/// Installs all required packages and returns package information to python
/// Takes the python script that is run and returns the virtual_sprawl root and a list of
/// virtual_sprawl modules (name, python_version, unique_version)
#[pyfunction]
fn prepare_virtual_sprawl(
    py: Python,
    file_running: &str,
    extras: Vec<String>,
    pep508_env: &str,
) -> PyResult<(String, Vec<(String, String, String)>)> {
    debug!("file for virtual sprawl: {}", file_running);
    let sys_executable: String = py.import("sys")?.getattr("executable")?.extract()?;

    let virtual_sprawl = setup_virtual_sprawl(
        Path::new(file_running),
        Path::new(&sys_executable),
        (py.version_info().major, py.version_info().minor),
        &extras,
        &Pep508Environment::from_json_str(pep508_env),
    );
    virtual_sprawl.map_err(|err| {
        let mut accumulator = "virtual sprawl failed to load.".to_string();
        for cause in err.chain().collect::<Vec<_>>().iter() {
            accumulator.push_str(&format!("\n  Caused by: {}", cause));
        }
        PyRuntimeError::new_err(accumulator)
    })
}

#[pymodule]
fn virtual_sprawl(_py: Python, m: &PyModule) -> PyResult<()> {
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
    m.add_function(wrap_pyfunction!(prepare_virtual_sprawl, m)?)?;
    Ok(())
}
