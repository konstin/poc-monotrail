use crate::markers::Pep508Environment;
use crate::requirements_txt::requirements_txt_to_specs;
use crate::spec::RequestedSpec;
use crate::{compatible_tags, install_specs, read_poetry_specs, Arch, InstallLocation, Os};
use anyhow::Context;
use fs_err as fs;
use std::env::current_dir;
use std::io;
use std::path::{Path, PathBuf};

pub fn virtual_sprawl_root() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from("/home/konsti/virtual_sprawl/virtual_sprawl"))
}

enum LockfileType {
    PyprojectToml,
    RequirementsTxt,
}

fn find_lockfile(file_running: &Path) -> Option<(PathBuf, LockfileType)> {
    let mut parent = if file_running.is_absolute() {
        file_running.parent().map(|path| path.to_path_buf())
    } else {
        file_running.parent().map(|relative| {
            current_dir()
                .unwrap_or_else(|_| PathBuf::from(""))
                .join(relative)
        })
    };

    while let Some(dir) = parent {
        if dir.join("pyproject.toml").exists() {
            return Some((dir.join("pyproject.toml"), LockfileType::PyprojectToml));
        }
        if dir.join("requirements-frozen.txt").exists() {
            return Some((
                dir.join("requirements-frozen.txt"),
                LockfileType::RequirementsTxt,
            ));
        }
        parent = dir.parent().map(|path| path.to_path_buf());
    }
    None
}

#[allow(clippy::type_complexity)]
pub fn filter_installed(
    specs: &[RequestedSpec],
    virtual_sprawl_root: &Path,
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<(String, String, String)>)> {
    let read_dir = match fs::read_dir(Path::new(&virtual_sprawl_root)) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok((specs.to_vec(), Vec::new()))
        }
        Err(err) => return Err(err).context("Failed to access virtual sprawl directory"),
    };
    let installed_packages: Vec<_> = read_dir
        .filter_map(|dir| dir.ok())
        .filter_map(|dir| {
            let filename = dir.file_name();
            let (name, version) = filename.to_str()?.split_once('-')?;
            Some((name.to_string(), version.to_string(), dir.path()))
        })
        .collect();

    let mut installed = Vec::new();
    let mut not_installed = Vec::new();
    for spec in specs {
        let unique_version = if let Some(source) = &spec.source {
            Some(source.resolved_reference.clone())
        } else {
            spec.python_version.as_ref().cloned()
        };

        if let Some(unique_version) = unique_version {
            if installed_packages.iter().any(|(name, version, _path)| {
                name == &spec.normalized_name() && version == &unique_version
            }) {
                installed.push((
                    spec.normalized_name(),
                    spec.python_version
                        .clone()
                        .context("TODO: needs python version")?,
                    unique_version,
                ));
            } else {
                not_installed.push(spec.clone());
            }
        } else {
            // For now we just take any version there is
            // This would take proper version resolution to make sense
            if let Some((name, unique_version, _path)) = installed_packages
                .iter()
                .find(|(name, _version, _path)| name == &spec.normalized_name())
            {
                installed.push((
                    name.to_string(),
                    spec.python_version
                        .clone()
                        .context("TODO: needs python version")?,
                    unique_version.to_string(),
                ));
            } else {
                not_installed.push(spec.clone());
            }
        }
    }

    Ok((not_installed, installed))
}

/// Returns a list name, python version, unique version
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn setup_virtual_sprawl(
    file_running: &Path,
    python: &Path,
    python_version: (u8, u8),
    extras: &[String],
    pep508_env: &Pep508Environment,
) -> anyhow::Result<(String, Vec<(String, String, String)>)> {
    let virtual_sprawl_root = virtual_sprawl_root()?;
    let (lockfile, lockfile_type) = find_lockfile(file_running).with_context(|| {
        format!(
            "pyproject.toml not found next to {} nor in any parent directory",
            file_running.display()
        )
    })?;
    let compatible_tags = compatible_tags(python_version, &Os::current()?, &Arch::current()?)?;
    let specs = match lockfile_type {
        LockfileType::PyprojectToml => read_poetry_specs(&lockfile, false, extras, &pep508_env)?,
        LockfileType::RequirementsTxt => {
            let requirements_txt = fs::read_to_string(&lockfile)?;
            requirements_txt_to_specs(&requirements_txt).with_context(|| {
                format!(
                    "requirements specification is invalid: {}",
                    lockfile.display()
                )
            })?
        }
    };

    let (to_install_specs, installed_done) =
        filter_installed(&specs, Path::new(&virtual_sprawl_root))?;

    let mut installed = install_specs(
        &to_install_specs,
        &InstallLocation::VirtualSprawl {
            virtual_sprawl_root: PathBuf::from(&virtual_sprawl_root),
            python: python.to_path_buf(),
            python_version,
        },
        &compatible_tags,
        false,
        true,
    )?;

    installed.extend(installed_done);

    let packages = installed
        .into_iter()
        .map(|(name, python_version, unique_version)| {
            (
                name.to_lowercase().replace('-', "_"),
                python_version,
                unique_version,
            )
        })
        .collect();

    let virtual_sprawl_location_string = virtual_sprawl_root
        .to_str()
        .context("virtual sprawl path is cursed")?
        .to_string();
    Ok((virtual_sprawl_location_string, packages))
}
