use crate::monotrail::{install, run_command_finder_data, PythonContext};
use crate::poetry_integration::lock::poetry_resolve_from_dir;
use crate::poetry_integration::poetry_toml;
use crate::poetry_integration::poetry_toml::PoetryPyprojectToml;
use crate::poetry_integration::read_dependencies::read_toml_files;
use crate::standalone_python::provision_python;
use crate::utils::data_local_dir;
use crate::{parse_major_minor, read_poetry_specs, DEFAULT_PYTHON_VERSION};
use anyhow::Context;
use fs_err as fs;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tempfile::TempDir;
use tracing::{debug, info};

/// Simple pipx reimplementation
///
/// Resolves one package, saving it in .local and runs one command from it
pub fn ppipx(
    package: Option<&str>,
    python_version: Option<&str>,
    version: Option<&str>,
    extras: &[String],
    command: &str,
    args: &[String],
) -> anyhow::Result<i32> {
    let python_version = python_version
        .map(parse_major_minor)
        .transpose()?
        .unwrap_or(DEFAULT_PYTHON_VERSION);

    let (python_context, python_home) = provision_python(python_version)?;
    let package = package.unwrap_or(command);
    let package_extras = if extras.is_empty() {
        package.to_string()
    } else {
        format!("{}[{}]", package, extras.join(","))
    };

    let resolution_dir = data_local_dir()?
        .join("ppipx")
        .join(&package_extras)
        .join(version.unwrap_or("latest"));

    if !resolution_dir.join("poetry.lock").is_file() {
        info!(
            "Generating ppipx entry for {}@{}",
            package_extras,
            version.unwrap_or("latest")
        );
        generate_ppipx_entry(
            version,
            extras,
            python_version,
            &python_context,
            package,
            &resolution_dir,
        )?;
    } else {
        debug!("ppipx entry already present")
    }

    let (poetry_section, poetry_lock, lockfile) = read_toml_files(&resolution_dir)
        .with_context(|| format!("Invalid ppipx entry at {}", resolution_dir.display()))?;
    let specs = read_poetry_specs(
        &poetry_section,
        poetry_lock,
        true,
        &[],
        &python_context.pep508_env,
    )?;

    let finder_data = install(&specs, BTreeMap::new(), lockfile, None, &python_context)
        .context("Couldn't install packages")?;

    run_command_finder_data(&command, &args, &python_context, &python_home, &finder_data)
}

/// Writes a pyproject.toml for the ppipx command and calls poetry to resolve it to a poetry.lock
fn generate_ppipx_entry(
    version: Option<&str>,
    extras: &[String],
    python_version: (u8, u8),
    python_context: &PythonContext,
    package: &str,
    resolution_dir: &PathBuf,
) -> anyhow::Result<()> {
    let mut dependencies = BTreeMap::new();
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
    if extras.is_empty() {
        dependencies.insert(
            package.to_string(),
            poetry_toml::Dependency::Compact(version.unwrap_or("*").to_string()),
        );
    } else {
        dependencies.insert(
            package.to_string(),
            poetry_toml::Dependency::Expanded {
                version: Some(version.unwrap_or("*").to_string()),
                optional: None,
                extras: Some(extras.to_vec()),
                git: None,
                branch: None,
            },
        );
    }
    let pyproject_toml = PoetryPyprojectToml {
        tool: Some(poetry_toml::ToolSection {
            poetry: Some(poetry_toml::PoetrySection {
                name: format!("{}_launcher", package),
                version: "0.0.1".to_string(),
                description: format!("Launcher for {}@{}", package, version.unwrap_or("latest")),
                authors: vec!["monotrail".to_string()],
                dependencies,
                dev_dependencies: Default::default(),
                extras: None,
                scripts: None,
            }),
        }),
        build_system: None,
    };

    fs::create_dir_all(&resolution_dir).context("Failed to create ppipx resolution dir")?;
    let resolve_dir = TempDir::new()?;
    fs::write(
        resolve_dir.path().join("pyproject.toml"),
        toml::to_string(&pyproject_toml).context("Failed to serialize pyproject.toml for ppipx")?,
    )?;
    poetry_resolve_from_dir(&resolve_dir, &python_context)?;
    fs::copy(
        resolve_dir.path().join("pyproject.toml"),
        resolution_dir.join("pyproject.toml"),
    )
    .context("Failed to copy ppipx pyproject.toml")?;
    fs::copy(
        resolve_dir.path().join("poetry.lock"),
        resolution_dir.join("poetry.lock"),
    )
    .context("Poetry didn't generate a poetry.lock")?;

    Ok(())
}
