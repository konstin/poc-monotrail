//! Test running the `monotrail` binary

use anyhow::bail;
use fs_err as fs;
use std::path::Path;
use std::process::{Command, Output};
use std::{io, str};

const BIN: &str = env!("CARGO_BIN_EXE_monotrail");

/// Returns the stdout lines of the successful process
fn handle_output(output: io::Result<Output>) -> anyhow::Result<Vec<String>> {
    match output {
        Ok(output) => {
            if !output.status.success() {
                bail!(
                    "Command failed: {}\n---stdout:\n{}\n---stderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            let stdout = str::from_utf8(&output.stdout)?;
            Ok(stdout.lines().map(ToString::to_string).collect())
        }
        Err(err) => Err(err.into()),
    }
}

#[test]
fn test_datascience() {
    for version in ["3.8", "3.9", "3.10"] {
        let output = Command::new(BIN)
            .args([
                "run",
                "-p",
                version,
                "python",
                "data_science_project/import_pandas.py",
            ])
            .output();
        let output = handle_output(output).unwrap();

        // contains to ignore log messages
        assert!(output.contains(&"1.4.2".to_string()));
        // .venv/bin/monotrail_python data_science_project/make_paper.py
    }
}

#[test]
fn test_flipstring() {
    let output = Command::new(BIN)
        .args(["run", "python", "flipstring/flip.py", "hello world!"])
        // windows uses the cp encoding otherwise and then printing utf8 characters fails
        .env("PYTHONIOENCODING", "utf8")
        .output();
    let output = handle_output(output).unwrap();

    // contains to ignore log messages
    assert!(output.contains(&"¡pꞁɹoM oꞁꞁǝH".to_string()));
    // .venv/bin/monotrail_python data_science_project/make_paper.py
}

#[test]
fn test_tox() {
    let output = Command::new(BIN)
        .args([
            "run",
            "-p",
            "3.8",
            "-p",
            "3.9",
            "-p",
            "3.10",
            "python",
            "numpy_version.py",
        ])
        .current_dir("data_science_project")
        .output();
    let output = handle_output(output).unwrap();
    let hellos: Vec<&str> = output
        .iter()
        .filter(|line| line.starts_with("hi from"))
        .map(|x| x.as_str())
        .collect();

    assert_eq!(
        hellos,
        [
            "hi from python 3.8 and numpy 1.22.3",
            "hi from python 3.9 and numpy 1.22.3",
            "hi from python 3.10 and numpy 1.22.3",
        ]
    );
}

/// There's some trickery involved (`ExternalArgs`) in making clap ignore the first arguments, also
/// tests ppipx
#[test]
fn test_pipx_black_version() {
    let output = Command::new(BIN)
        .args([
            "ppipx",
            "--extras",
            "jupyter",
            "--version",
            "22.3.0",
            "black",
            "--version",
        ])
        .output();
    let output = handle_output(output).unwrap();
    assert!(output
        .last()
        .expect("Expected at least one line")
        .starts_with("black, 22.3.0"));
}

/// Tests the flat src layout and whether `python run` works without poetry.lock
#[test]
fn test_srcery() {
    let poetry_lock = Path::new("srcery").join("poetry.lock");
    if poetry_lock.is_file() {
        fs::remove_file(poetry_lock).unwrap();
    }
    let output = Command::new(BIN)
        .args([
            "run",
            "python",
            "-c",
            "from srcery import satanarchaeolidealcohellish_notion_potion; print(satanarchaeolidealcohellish_notion_potion())",
        ]).current_dir("srcery")
        .output();
    let output = handle_output(output).unwrap();
    assert_eq!(
        output.last().expect("Expected at least one line"),
        "https://www.youtube.com/watch?v=D5YYoY9l9Ew"
    );
}

/// Test our poetry runner in general and specifically the in-project-venv setting that
/// we need if poetry creates venv when it shouldn't so they stay in the tmp dir and get cleaned up
#[test]
fn test_poetry_config() {
    let output = Command::new(BIN)
        .args(["poetry", "config", "--list"])
        .env("POETRY_VIRTUALENVS_IN_PROJECT", "1")
        .output();
    let output = handle_output(output).unwrap();
    let line = output
        .iter()
        .find(|line| line.starts_with("virtualenvs.in-project"))
        .expect("Expected virtualenvs.in-project");
    assert_eq!(line, "virtualenvs.in-project = true");
}
