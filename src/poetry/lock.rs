//! calls to poetry to resolve a set of requirements

use crate::poetry::{poetry_lock, poetry_toml};
use anyhow::{bail, Context};
use std::collections::HashMap;
use std::default::Default;
use std::process::Command;
use std::{fs, io};
use tempfile::tempdir;

/// Calls poetry to resolve the user specified dependencies into a set of locked consistent
/// dependencies. Produces a poetry.lock in the process
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn resolve(
    mut dependencies: HashMap<String, poetry_toml::Dependency>,
    python_version: (u8, u8),
) -> anyhow::Result<(poetry_toml::PoetryPyprojectToml, poetry_lock::PoetryLock)> {
    // Add python entry with current version; resolving will otherwise fail with complaints
    dependencies.insert(
        "python".to_string(),
        // For some reason on github actions 3.8.12 is not 3.8 compatible, so we name the range explicitly
        poetry_toml::Dependency::Compact(format!(
            ">={}.{},<{}.{}",
            python_version.0,
            python_version.1,
            python_version.0,
            python_version.1 + 1
        )),
    );
    // build dummy poetry pyproject.toml
    let pyproject_toml_content = poetry_toml::PoetryPyprojectToml {
        tool: poetry_toml::ToolSection {
            poetry: poetry_toml::PoetrySection {
                name: "monotrail_dummy_project_for_locking".to_string(),
                version: "1.0.0".to_string(),
                description: "monotrail generated this dummy pyproject.toml to call poetry and let it do the dependency resolution".to_string(),
                authors: vec!["konstin <konstin@mailbox.org>".to_string()],
                dependencies,
                dev_dependencies: HashMap::new(),
                extras: HashMap::new(),
            },
        },
        build_system: Default::default()
    };
    // Write complete dummy poetry pyproject.toml
    let resolve_dir = tempdir()?;
    let pyproject_toml_path = resolve_dir.path().join("pyproject.toml");
    fs::write(&pyproject_toml_path, toml::to_vec(&pyproject_toml_content)?)?;
    // Call poetry to resolve dependencies. This will generate `poetry.lock` in the same directory
    let result = Command::new("poetry")
        .args(&["lock", "--no-update"])
        .current_dir(&resolve_dir)
        .status();
    match result {
        Ok(status) if status.success() => {
            // we're good
        }
        Ok(status) => {
            bail!(
                "Poetry's dependency resolution errored: {}. Please check the log above",
                status
            )
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            bail!("Could not find poetry. Is it installed and in PATH? https://python-poetry.org/docs/#installation")
        }
        Err(err) => {
            return Err(err).context("Failed to run poetry to resolve dependencies");
        }
    }
    // read back the pyproject.toml and compare, just to be sure
    let pyproject_toml_reread = toml::from_str(&fs::read_to_string(pyproject_toml_path)?)?;
    if pyproject_toml_content != pyproject_toml_reread {
        bail!("Consistency check failed: pyproject.toml we read is no the one we wrote");
    }
    // read poetry lock with the dependencies resolved by poetry
    let poetry_lock = toml::from_str(&fs::read_to_string(resolve_dir.path().join("poetry.lock"))?)?;

    Ok((pyproject_toml_content, poetry_lock))
}
