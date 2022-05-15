//! calls to poetry to resolve a set of requirements

use crate::inject_and_run::inject_and_run_python;
use crate::markers::Pep508Environment;
use crate::monotrail::{install_requested, install_specs_to_finder};
use crate::package_index::cache_dir;
use crate::poetry_integration::poetry_lock::PoetryLock;
use crate::poetry_integration::poetry_toml;
use crate::poetry_integration::poetry_toml::PoetryPyprojectToml;
use crate::poetry_integration::read_dependencies::poetry_spec_from_dir;
use crate::read_poetry_specs;
use anyhow::{bail, Context};
use fs_err as fs;
use std::collections::HashMap;
use std::default::Default;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use std::{env, io};
use tempfile::tempdir;
use tracing::{debug, span, Level};

/// Minimal dummy pyproject.toml with the user requested deps for poetry to resolve
fn dummy_poetry_pyproject_toml(
    mut dependencies: HashMap<String, poetry_toml::Dependency>,
    python_version: (u8, u8),
) -> poetry_toml::PoetryPyprojectToml {
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
        tool: poetry_toml::ToolSection {
            poetry: poetry_toml::PoetrySection {
                name: "monotrail_dummy_project_for_locking".to_string(),
                version: "1.0.0".to_string(),
                description: "monotrail generated this dummy pyproject.toml to call poetry and let it do the dependency resolution".to_string(),
                authors: vec!["konstin <konstin@mailbox.org>".to_string()],
                dependencies,
                dev_dependencies: HashMap::new(),
                extras: Some(HashMap::new()),
                scripts: None,
            },
        },
        build_system: Default::default()
    }
}

/// Calls poetry to resolve the user specified dependencies into a set of locked consistent
/// dependencies. Produces a poetry.lock in the process
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn poetry_resolve(
    dependencies: HashMap<String, poetry_toml::Dependency>,
    sys_executable: &Path,
    python_version: (u8, u8),
    lockfile: Option<&str>,
    pep508_env: &Pep508Environment,
    // whether to use `python -m monotrail.run` or `monotrail poetry-resolve`. The former is used
    // when running with a standalone python, the latter when running from binary. We pass the root
    // of the
    python_root: Option<PathBuf>,
) -> anyhow::Result<(PoetryPyprojectToml, PoetryLock, String)> {
    // Write a dummy poetry pyproject.toml with the requested dependencies
    let resolve_dir = tempdir()?;
    let pyproject_toml_content = dummy_poetry_pyproject_toml(dependencies, python_version);
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

    // The new process we spawn would also do this, but this way we get better debuggability
    let bootstrapping_span = span!(Level::DEBUG, "bootstrapping_poetry");
    let (specs, _scripts, _old_lockfile) =
        poetry_spec_from_dir(&poetry_boostrap_lock, &[], pep508_env)?;
    install_requested(&specs, Path::new(&sys_executable), python_version)
        .context("Failed to bootstrap poetry")?;
    drop(bootstrapping_span);

    debug!("resolving with poetry");
    let resolve_span = span!(Level::DEBUG, "resolving_with_poetry");
    let start = Instant::now();
    let result = if let Some(python_root) = &python_root {
        // First argument must always be the program itself
        Command::new(env::current_exe()?)
            .arg("poetry-resolve")
            .arg(python_version.0.to_string())
            .arg(python_version.1.to_string())
            .arg(&python_root)
            // This will make poetry-resolve find the pyproject.toml we want to resolve
            .current_dir(&resolve_dir)
            .status()
    } else {
        Command::new(sys_executable)
            .args(["-m", "monotrail.run", "poetry", "lock", "--no-update"])
            // This will make the monotrail python part find the poetry lock for poetry itself
            .env(
                format!("{}_CWD", env!("CARGO_PKG_NAME")).to_uppercase(),
                &poetry_boostrap_lock,
            )
            // This will make poetry lock the right deps
            .current_dir(&resolve_dir)
            .status()
    };
    drop(resolve_span);
    debug!(
        "poetry lock took {:.2}s",
        (Instant::now() - start).as_secs_f32()
    );

    match result {
        Ok(status) if status.success() => {
            // we're good
        }
        Ok(status) => {
            if python_root.is_some() {
                bail!("Recursive invocation to resolve dependencies failed: {}. Please check the log above", status);
            } else {
                bail!(
                    "Poetry's dependency resolution errored: {}. Please check the log above",
                    status
                );
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound && python_root.is_some() => {
            bail!("Could not find poetry. Is it installed and in PATH? https://python-poetry.org/docs/#installation")
        }
        Err(err) => {
            return Err(err).context("Failed to run poetry to resolve dependencies");
        }
    }
    // read back the pyproject.toml and compare, just to be sure
    let pyproject_toml_reread = toml::from_str(&fs::read_to_string(pyproject_toml_path)?)?;
    if pyproject_toml_content != pyproject_toml_reread {
        bail!("Consistency check failed: the pyproject.toml we read is not the one we wrote");
    }
    let lockfile = fs::read_to_string(poetry_lock_path)?;
    // read poetry lock with the dependencies resolved by poetry
    let poetry_lock = toml::from_str(&fs::read_to_string(resolve_dir.path().join("poetry.lock"))?)?;

    Ok((pyproject_toml_content, poetry_lock, lockfile))
}

pub fn poetry_resolve_bin(major: u8, minor: u8, python_root: &Path) -> anyhow::Result<()> {
    let python_binary = python_root.join("install").join("bin").join("python3");
    let python_version = (major, minor);
    let pep508_env = Pep508Environment::from_python(&python_binary);

    let pyproject_toml = include_str!("poetry_boostrap_lock/pyproject.toml");
    let poetry_toml: PoetryPyprojectToml = toml::from_str(pyproject_toml).unwrap();
    let lockfile = include_str!("poetry_boostrap_lock/poetry.lock");
    let poetry_lock: PoetryLock = toml::from_str(lockfile).unwrap();

    let scripts = poetry_toml.tool.poetry.scripts.clone().unwrap_or_default();
    let specs = read_poetry_specs(poetry_toml, poetry_lock, true, &[], &pep508_env)?;

    let finder_data = install_specs_to_finder(
        &specs,
        python_binary.to_string_lossy().to_string(),
        python_version,
        scripts,
        lockfile.to_string(),
        None,
    )
    .context("Failed to bootstrap poetry")?;

    let temp_dir = tempdir()?;
    let main_file = temp_dir.path().join("poetry_launcher.py");
    std::fs::write(&main_file, "from poetry.console import main\nmain()")?;
    inject_and_run_python(
        python_root,
        &[
            python_binary.to_string_lossy().to_string(),
            main_file.to_string_lossy().to_string(),
            "lock".to_string(),
        ],
        &serde_json::to_string(&finder_data)?,
    )
    .context("Running poetry for dependency resolution failed")?;
    Ok(())
}
