use crate::PEP508_QUERY_ENV;
use pep508_rs::MarkerEnvironment;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// If we launch from python, we can call the python code from python with no overhead, but
/// still need to parse into Self here
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn marker_environment_from_json_str(pep508_env_data: &str) -> MarkerEnvironment {
    serde_json::from_str(pep508_env_data).unwrap()
}

/// Runs python to get the actual PEP 508 values
///
/// To be eventually replaced by something like the maturin solution where we construct this
/// is in rust
pub fn marker_environment_from_python(python: &Path) -> MarkerEnvironment {
    let out = Command::new(python)
        .args(["-S"])
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            // We only have the module definition in that file (because we also want to load
            // it as a module in the python bindings), so we need to append the actual call
            let pep508_query_script = format!("{}\nprint(get_pep508_env())", PEP508_QUERY_ENV);
            child
                .stdin
                .as_mut()
                .expect("piped stdin")
                .write_all(pep508_query_script.as_bytes())?;
            child.wait_with_output()
        });

    let returned = match out {
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                panic!(
                    "Could not find any interpreter at {}, \
                        are you sure you have Python installed on your PATH?",
                    python.display()
                )
            } else {
                panic!(
                    "Failed to run the Python interpreter at {}: {}",
                    python.display(),
                    err
                )
            }
        }
        Ok(ok) if !ok.status.success() => panic!("Python script failed"),
        Ok(ok) => ok.stdout,
    };
    serde_json::from_slice(&returned).unwrap()
}
