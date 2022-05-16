use crate::inject_and_run::{inject_and_run_python, parse_plus_arg};
use crate::monotrail::{install_specs_to_finder, LaunchType, PythonContext};
use crate::poetry_integration::poetry_lock::PoetryLock;
use crate::poetry_integration::poetry_toml::PoetryPyprojectToml;
use crate::standalone_python::provision_python;
use crate::{read_poetry_specs, Pep508Environment};
use anyhow::Context;
use tempfile::tempdir;

/// Use the libpython.so to run a poetry command on python 3.8, unless you give +x.y as first
/// argument
pub fn poetry_run(args: Vec<String>) -> anyhow::Result<()> {
    let (args, python_version) = parse_plus_arg(args)?;
    let python_version = python_version.unwrap_or((3, 8));
    let python_root = provision_python(python_version)?;
    let python_binary = python_root.join("install").join("bin").join("python3");
    let pep508_env = Pep508Environment::from_python(&python_binary);

    let pyproject_toml = include_str!("poetry_boostrap_lock/pyproject.toml");
    let poetry_toml: PoetryPyprojectToml = toml::from_str(pyproject_toml).unwrap();
    let lockfile = include_str!("poetry_boostrap_lock/poetry.lock");
    let poetry_lock: PoetryLock = toml::from_str(lockfile).unwrap();

    let scripts = poetry_toml.tool.poetry.scripts.clone().unwrap_or_default();
    let specs = read_poetry_specs(poetry_toml, poetry_lock, true, &[], &pep508_env)?;

    let python_context = PythonContext {
        sys_executable: python_binary,
        python_version,
        pep508_env,
        launch_type: LaunchType::Binary,
    };

    let finder_data =
        install_specs_to_finder(&specs, scripts, lockfile.to_string(), None, &python_context)
            .context("Failed to bootstrap poetry")?;

    let temp_dir = tempdir()?;
    let main_file = temp_dir.path().join("poetry_launcher.py");
    std::fs::write(&main_file, "from poetry.console import main\nmain()")?;
    let poetry_args: Vec<_> = [
        python_context.sys_executable.to_string_lossy().to_string(),
        main_file.to_string_lossy().to_string(),
    ]
    .into_iter()
    .chain(args)
    .collect();

    inject_and_run_python(
        &python_root,
        &poetry_args,
        &serde_json::to_string(&finder_data)?,
    )
    .context("Running poetry for dependency resolution failed")?;
    Ok(())
}
