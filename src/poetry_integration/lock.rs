//! calls to poetry to resolve a set of requirements

use crate::monotrail::{install_missing, LaunchType, PythonContext};
use crate::poetry_integration::poetry_lock::PoetryLock;
use crate::poetry_integration::poetry_toml;
use crate::poetry_integration::poetry_toml::{PoetryPyprojectToml, PoetrySection};
use crate::poetry_integration::read_dependencies::read_toml_files;
use crate::read_poetry_specs;
use crate::utils::cache_dir;
use anyhow::{bail, format_err, Context};
use fs_err as fs;
use std::collections::BTreeMap;
use std::default::Default;
use std::process::Command;
use std::time::Instant;
use std::{env, io};
use tempfile::{tempdir, TempDir};
use tracing::{debug, span, Level};

/// Minimal dummy pyproject.toml with the user requested deps for poetry to resolve
pub fn dummy_poetry_pyproject_toml(
    dependencies: &BTreeMap<String, poetry_toml::Dependency>,
    python_version: (u8, u8),
) -> PoetryPyprojectToml {
    let mut dependencies = dependencies.clone();
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
    PoetryPyprojectToml {
        tool: Some(poetry_toml::ToolSection {
            poetry: Some(PoetrySection {
                name: "monotrail_dummy_project_for_locking".to_string(),
                version: "1.0.0".to_string(),
                description: "monotrail generated this dummy pyproject.toml to call poetry and let it do the dependency resolution".to_string(),
                authors: vec!["konstin <konstin@mailbox.org>".to_string()],
                dependencies,
                dev_dependencies: None,
                extras: Some(BTreeMap::new()),
                scripts: None,
            }),
        }),
        build_system: Default::default()
    }
}

/// Calls poetry to resolve the user specified dependencies into a set of locked consistent
/// dependencies. Produces a poetry.lock in the process
pub fn poetry_resolve(
    dependencies: &BTreeMap<String, poetry_toml::Dependency>,
    lockfile: Option<&str>,
    python_context: &PythonContext,
) -> anyhow::Result<(PoetrySection, PoetryLock, String)> {
    // Write a dummy poetry pyproject.toml with the requested dependencies
    let resolve_dir = tempdir()?;
    let pyproject_toml_content = dummy_poetry_pyproject_toml(dependencies, python_context.version);
    let pyproject_toml_path = resolve_dir.path().join("pyproject.toml");
    fs::write(
        &pyproject_toml_path,
        toml::to_string(&pyproject_toml_content).context("Failed to write pyproject.toml")?,
    )?;
    // If we have a previous lockfile, we want to reuse it for two reasons:
    // * if there wasn't any change in requirements, we don't need to do any resolution
    // * if requirements changed, we want to maximize the chance of not changing versions
    //   and minimize the resolution work
    let poetry_lock_path = resolve_dir.path().join("poetry.lock");
    if let Some(lockfile) = lockfile {
        fs::write(&poetry_lock_path, &lockfile)?;
    }

    poetry_resolve_from_dir(&resolve_dir, &python_context)?;
    // read back the pyproject.toml and compare, just to be sure
    let pyproject_toml_reread = toml::from_str(&fs::read_to_string(pyproject_toml_path)?)?;
    if pyproject_toml_content != pyproject_toml_reread {
        bail!("Consistency check failed: the pyproject.toml we read is not the one we wrote");
    }
    let lockfile = fs::read_to_string(poetry_lock_path)?;
    // read poetry lock with the dependencies resolved by poetry
    let poetry_lock =
        PoetryLock::from_str(&fs::read_to_string(resolve_dir.path().join("poetry.lock"))?)?;

    let poetry_section = pyproject_toml_content.tool.unwrap().poetry.unwrap();
    Ok((poetry_section, poetry_lock, lockfile))
}

/// Runs `poetry lock --no-update` in the given tempdir, which needs to contain a pyproject.toml
/// and optionally a poetry.lock
pub fn poetry_resolve_from_dir(
    resolve_dir: &TempDir,
    python_context: &PythonContext,
) -> anyhow::Result<()> {
    // Setup a directory with the dependencies of poetry itself, so we can run poetry in a
    // recursive call to monotrail through the python interface.
    // Poetry internally is normally installed through get-poetry.py which creates a new virtualenv
    // and then just call pip with the pypi version, so we can install and use through our own
    // mechanism.
    // Maybe it would more elegant to do this through pyo3, not sure.
    let bootstrapping_span = span!(Level::DEBUG, "bootstrapping_poetry");
    let plus_version = format!("+{}.{}", python_context.version.0, python_context.version.1);
    let poetry_boostrap_lock = cache_dir()?.join("poetry_boostrap_lock");
    fs::create_dir_all(&poetry_boostrap_lock)?;
    fs::write(
        poetry_boostrap_lock.join("poetry.lock"),
        include_str!("../../resources/poetry_boostrap_lock/poetry.lock"),
    )?;
    fs::write(
        poetry_boostrap_lock.join("pyproject.toml"),
        include_str!("../../resources/poetry_boostrap_lock/pyproject.toml"),
    )?;

    let (poetry_section, poetry_lock, _lockfile) = read_toml_files(&poetry_boostrap_lock)
        .context("Failed to read toml files that were just written")?;
    let specs = read_poetry_specs(
        &poetry_section,
        poetry_lock,
        false,
        &[],
        &python_context.pep508_env,
    )?;
    install_missing(
        &specs,
        &python_context.sys_executable,
        python_context.version,
    )
    .context("Failed to bootstrap poetry")?;
    drop(bootstrapping_span);

    // If poetry creates venv (which it shouldn't, but sometimes does anyway), which want it to do
    // that inside the tempdir we empty later
    // TODO: Do we need to scope this?
    env::set_var("POETRY_VIRTUALENVS_IN_PROJECT", "1");

    let resolve_span = span!(Level::DEBUG, "resolving_with_poetry");
    let start = Instant::now();
    let (result, command, args) = match python_context.launch_type {
        LaunchType::Binary => {
            debug!("resolving with poetry (binary)");
            // First argument must always be the program itself
            let current_exe = env::current_exe()?;
            let args = vec!["poetry", &plus_version, "lock", "--no-update"];
            let result = Command::new(&current_exe)
                .args(&args)
                // This will make poetry-resolve find the pyproject.toml we want to resolve
                .current_dir(&resolve_dir)
                .status();
            (result, current_exe, args)
        }
        LaunchType::PythonBindings => {
            debug!("resolving with poetry (python bindings)");
            let args = vec![
                "-m",
                "monotrail.run_script",
                "poetry",
                "lock",
                "--no-update",
            ];
            let result = Command::new(&python_context.sys_executable)
                .args(&args)
                // This will make the monotrail python part find the poetry lock for poetry itself
                .env(
                    format!("{}_CWD", env!("CARGO_PKG_NAME")).to_uppercase(),
                    &poetry_boostrap_lock,
                )
                // This will make poetry lock the right deps
                .current_dir(&resolve_dir)
                .status();
            (result, python_context.sys_executable.clone(), args)
        }
    };
    drop(resolve_span);
    debug!(
        "poetry lock took {:.2}s",
        (Instant::now() - start).as_secs_f32()
    );

    match result {
        Ok(status) if status.success() => {
            // we're good
            Ok(())
        }
        Ok(status) => {
            Err(format_err!(
                "Failed to run poetry in {}: `{} {}`",
                resolve_dir.path().display(),
                command.display(),
                args.join(" "),
            )).context(format!(
                "Recursive invocation to resolve dependencies failed with {}. Please check the log above.",
                status,
            ))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound && python_context.launch_type == LaunchType::PythonBindings => {
            Err(format_err!("Could not find poetry. Is it installed and in PATH? https://python-poetry.org/docs/#installation"))
        }
        Err(err) => {
            Err(err).context(format!(
                "Failed to run poetry to resolve dependencies in {}: {} {}", 
                resolve_dir.path().display(), command.display(), args.join(" ")
            ))
        }
    }
}
