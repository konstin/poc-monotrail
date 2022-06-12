//! Runs poetry after installing it from a bundle lockfile

use crate::inject_and_run::{determine_python_version, inject_and_run_python};
use crate::monotrail::install;
use crate::poetry_integration::poetry_lock::PoetryLock;
use crate::poetry_integration::poetry_toml::PoetryPyprojectToml;
use crate::read_poetry_specs;
use crate::standalone_python::provision_python;
use anyhow::Context;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Use the libpython.so to run a poetry command on python 3.8, unless you give +x.y as first
/// argument
pub fn poetry_run(args: &[String], python_version: Option<&str>) -> anyhow::Result<i32> {
    let (args, python_version) = determine_python_version(&args, python_version)?;
    let (python_context, python_home) = provision_python(python_version)?;

    let pyproject_toml = include_str!("poetry_boostrap_lock/pyproject.toml");
    let poetry_toml: PoetryPyprojectToml = toml::from_str(pyproject_toml).unwrap();
    let lockfile = include_str!("poetry_boostrap_lock/poetry.lock");
    let poetry_lock: PoetryLock = toml::from_str(lockfile).unwrap();

    let poetry_section = poetry_toml.tool.unwrap().poetry.unwrap();
    let specs = read_poetry_specs(
        &poetry_section,
        poetry_lock,
        true,
        &[],
        &python_context.pep508_env,
    )?;

    let finder_data = install(
        &specs,
        BTreeMap::new(),
        lockfile.to_string(),
        None,
        &python_context,
    )
    .context("Failed to bootstrap poetry")?;

    let poetry_package = finder_data
        .sprawl_packages
        .iter()
        .find(|package| package.name == "poetry")
        .context("poetry is missing 🤨")?;
    let launcher = poetry_package
        .monotrail_location(PathBuf::from(&finder_data.sprawl_root))
        .join("bin")
        .join("poetry");

    let poetry_args: Vec<_> = [
        python_context.sys_executable.to_string_lossy().to_string(),
        launcher.to_string_lossy().to_string(),
    ]
    .into_iter()
    .chain(args)
    .collect();

    let exit_code = inject_and_run_python(
        &python_home,
        python_version,
        // poetry doesn't need monotrail-moonlighting-as-python subprocesses
        // (at least i never encountered that)
        &python_context.sys_executable,
        &poetry_args,
        &serde_json::to_string(&finder_data)?,
    )
    .context("Running poetry for dependency resolution failed")?;
    Ok(exit_code as i32)
}
