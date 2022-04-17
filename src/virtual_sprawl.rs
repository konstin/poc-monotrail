use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::requirements_txt::requirements_txt_to_specs;
use crate::spec::RequestedSpec;
use crate::{compatible_tags, install_specs, read_poetry_specs, Arch, InstallLocation, Os};
use anyhow::Context;
use fs_err as fs;
use fs_err::DirEntry;
use std::env;
use std::env::current_dir;
use std::path::{Path, PathBuf};

pub fn virtual_sprawl_root() -> anyhow::Result<PathBuf> {
    if let Some(env_root) = env::var_os("VIRTUAL_SPRAWL_ROOT") {
        Ok(PathBuf::from(env_root))
    } else {
        Ok(PathBuf::from("/home/konsti/virtual_sprawl/virtual_sprawl"))
    }
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

fn get_dir_content(dir: &Path) -> anyhow::Result<Vec<DirEntry>> {
    let read_dir =
        fs::read_dir(Path::new(&dir)).context("Failed to load virtual sprawl directory")?;
    Ok(read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect())
}

pub fn filter_installed(
    specs: &[RequestedSpec],
    virtual_sprawl_root: &Path,
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    // Behold my monstrous iterator
    // name -> version -> compatible tag
    let installed_packages: Vec<(String, String, String)> = get_dir_content(virtual_sprawl_root)
        // No virtual sprawl dir, no packages
        .unwrap_or_default()
        .iter()
        .map(|name_dir| {
            Ok(get_dir_content(&name_dir.path())?
                .iter()
                .map(|version_dir| {
                    Ok(get_dir_content(&version_dir.path())?
                        .iter()
                        .map(|tag_dir| {
                            (
                                name_dir.file_name().to_string_lossy().to_string(),
                                version_dir.file_name().to_string_lossy().to_string(),
                                tag_dir.file_name().to_string_lossy().to_string(),
                            )
                        })
                        .collect::<Vec<(String, String, String)>>())
                })
                .collect::<anyhow::Result<Vec<Vec<(String, String, String)>>>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>())
        })
        .collect::<anyhow::Result<Vec<Vec<(String, String, String)>>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let mut installed = Vec::new();
    let mut not_installed = Vec::new();
    for spec in specs {
        let unique_version = if let Some(source) = &spec.source {
            Some(source.resolved_reference.clone())
        } else {
            spec.python_version.as_ref().cloned()
        };

        if let Some(unique_version) = unique_version {
            if let Some((name, installed_version, tag)) =
                installed_packages
                    .iter()
                    .find(|(name, installed_version, _tag)| {
                        name == &spec.normalized_name() && installed_version == &unique_version
                    })
            {
                installed.push(InstalledPackage {
                    name: name.clone(),
                    python_version: spec
                        .python_version
                        .clone()
                        .context("TODO: needs python version")?,
                    unique_version: installed_version.clone(),
                    tag: tag.clone(),
                });
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
                installed.push(InstalledPackage {
                    // already normalized
                    name: name.clone(),
                    python_version: spec
                        .python_version
                        .clone()
                        .context("TODO: needs python version")?,
                    unique_version: unique_version.to_string(),
                    tag: "".to_string(),
                });
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
) -> anyhow::Result<(String, Vec<InstalledPackage>)> {
    let virtual_sprawl_root = virtual_sprawl_root()?;
    let (lockfile, lockfile_type) = find_lockfile(file_running).with_context(|| {
        format!(
            "pyproject.toml not found next to {} nor in any parent directory",
            file_running.display()
        )
    })?;
    let compatible_tags = compatible_tags(python_version, &Os::current()?, &Arch::current()?)?;
    let specs = match lockfile_type {
        LockfileType::PyprojectToml => read_poetry_specs(&lockfile, false, extras, pep508_env)?,
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

    let virtual_sprawl_location_string = virtual_sprawl_root
        .to_str()
        .context("virtual sprawl path is cursed")?
        .to_string();
    Ok((virtual_sprawl_location_string, installed))
}
