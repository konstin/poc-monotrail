//! calls to poetry to resolve a set of requirements

use crate::package_index::cache_dir;
use crate::poetry::{poetry_lock, poetry_toml};
use anyhow::{bail, Context};
use fs_err as fs;
use std::collections::HashMap;
use std::default::Default;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tempfile::tempdir;
use tracing::debug;

/// Calls poetry to resolve the user specified dependencies into a set of locked consistent
/// dependencies. Produces a poetry.lock in the process
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn resolve(
    mut dependencies: HashMap<String, poetry_toml::Dependency>,
    sys_executable: &Path,
    python_version: (u8, u8),
    lockfile: Option<&str>,
) -> anyhow::Result<(
    poetry_toml::PoetryPyprojectToml,
    poetry_lock::PoetryLock,
    String,
)> {
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
                extras: Some(HashMap::new()),
            },
        },
        build_system: Default::default()
    };
    // Write complete dummy poetry pyproject.toml
    let resolve_dir = tempdir()?;
    let pyproject_toml_path = resolve_dir.path().join("pyproject.toml");
    fs::write(&pyproject_toml_path, toml::to_vec(&pyproject_toml_content)?)?;
    // If we have a previous lockfile, we want to reuse it for two reasons:
    // * if there wasn't any change in requirements, we don't need to do any resolution
    // * if requirements changed, we want to maximize the chance of not changing versions
    //   and minimize the resolution work
    let poetry_lock_path = resolve_dir.path().join("poetry.lock");
    if let Some(lockfile) = lockfile {
        fs::write(&poetry_lock_path, &lockfile)?;
    }

    // Setup a directory with the dependencies of poetry itself, so we can run poetry in a
    // recursive call to monotrail through the python interface.
    // Poetry internally is normally installed through get-poetry.py which creates a new virtualenv
    // and then just call pip with the pypi version, so we can install and use through our own
    // mechanism.
    // Maybe it would more elegant to do this through pyo3, not sure.
    let poetry_boostrap_lock = cache_dir()?.join("poetry_boostrap_lock");
    fs::create_dir_all(&poetry_boostrap_lock)?;
    fs::write(
        poetry_boostrap_lock.join("poetry.lock"),
        include_str!("poetry_boostrap_lock/poetry.lock"),
    )?;
    fs::write(
        poetry_boostrap_lock.join("pyproject.toml"),
        include_str!("poetry_boostrap_lock/pyproject.toml"),
    )?;

    let start = Instant::now();
    let result = Command::new(sys_executable)
        .args(["-m", "monotrail.run", "poetry", "lock", "--no-update"])
        // This will make poetry lock the right deps
        .current_dir(&resolve_dir)
        // This will make the monotrail python part find the poetry lock for poetry itself
        .env("MONOTRAIL", "1")
        .env("MONOTRAIL_CWD", &poetry_boostrap_lock)
        .status();
    debug!(
        "poetry lock took {:.2}s",
        (Instant::now() - start).as_secs_f32()
    );

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
    let lockfile = fs::read_to_string(poetry_lock_path)?;
    // read poetry lock with the dependencies resolved by poetry
    let poetry_lock = toml::from_str(&fs::read_to_string(resolve_dir.path().join("poetry.lock"))?)?;

    Ok((pyproject_toml_content, poetry_lock, lockfile))
}
