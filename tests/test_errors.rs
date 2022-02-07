//! Tests the error messages for broken wheels

use anyhow::{bail, Error, Result};
use clap::Parser;
use install_wheel_rs::{run, Cli};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn check_error(name: &str, expected: &[&str]) -> Result<()> {
    let temp_dir = TempDir::new()?;
    let venv = temp_dir.path().join(".venv");
    Command::new("virtualenv").arg(&venv).output()?;
    let wheel = Path::new("pip/tests/data/packages").join(name);
    let cli: Cli = Cli::try_parse_from(&[
        "install-wheel-rs",
        "install-files",
        &wheel.display().to_string(),
    ])?;
    if let Err(err) = run(cli, &venv) {
        let err: Error = err;
        let actual = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        assert_eq!(expected, actual);
    } else {
        bail!("Should have errored");
    }
    Ok(())
}

#[test]
fn test_brokenwheel() -> Result<()> {
    check_error(
        "brokenwheel-1.0-py2.py3-none-any.whl",
        &[
            "Failed to install pip/tests/data/packages/brokenwheel-1.0-py2.py3-none-any.whl",
            "The wheel is invalid: Inconsistent package name: simple.dist (wheel metadata) vs brokenwheel (filename)",
        ],
    )
}

#[test]
fn test_corruptwheel() -> Result<()> {
    check_error(
        "corruptwheel-1.0-py2.py3-none-any.whl",
        &[
            "Failed to install pip/tests/data/packages/corruptwheel-1.0-py2.py3-none-any.whl",
            "The wheel is broken",
            "invalid Zip archive",
            "invalid Zip archive",
        ],
    )
}

#[test]
fn test_invalid() -> Result<()> {
    check_error(
        "invalid.whl",
        &[
            "Failed to install pip/tests/data/packages/invalid.whl",
            "The wheel filename \"invalid.whl\" is invalid: Expected four \"-\" in the filename",
        ],
    )
}

#[test]
fn test_priority() -> Result<()> {
    check_error(
        "priority-1.0-py2.py3-none-any.whl",
        &[
            "Failed to install pip/tests/data/packages/priority-1.0-py2.py3-none-any.whl",
            "The wheel is broken",
            "invalid Zip archive",
            "invalid Zip archive",
        ],
    )
}