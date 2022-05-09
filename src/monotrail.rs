use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::package_index::cache_dir;
use crate::poetry_integration::lock::poetry_resolve;
use crate::poetry_integration::poetry_toml;
use crate::poetry_integration::read_dependencies::read_toml_files;
use crate::requirements_txt::parse_requirements_txt;
use crate::spec::RequestedSpec;
use crate::{install_specs, read_poetry_specs};
use anyhow::{bail, Context};
use fs_err as fs;
use fs_err::DirEntry;
use install_wheel_rs::{compatible_tags, Arch, InstallLocation, Os};
use std::collections::HashMap;
use std::env::current_dir;
use std::path::{Path, PathBuf};
use std::{env, io};
use tracing::{debug, warn};

pub fn monotrail_root() -> anyhow::Result<PathBuf> {
    if let Some(env_root) = env::var_os("MONOTRAIL_ROOT") {
        Ok(PathBuf::from(env_root))
    } else {
        Ok(cache_dir()?.join("monotrail"))
    }
}

enum LockfileType {
    PyprojectToml,
    RequirementsTxt,
}

/// Walks the directory tree up to find a pyproject.toml or a requirements.txt and returns
/// the dir (poetry) or the file (requirements.txt)
fn find_dep_file(dir_running: &Path) -> Option<(PathBuf, LockfileType)> {
    let mut parent = Some(dir_running.to_path_buf());
    while let Some(dir) = parent {
        if dir.join("pyproject.toml").exists() {
            return Some((dir, LockfileType::PyprojectToml));
        }
        if dir.join("requirements.txt").exists() {
            return Some((dir.join("requirements.txt"), LockfileType::RequirementsTxt));
        }
        parent = dir.parent().map(|path| path.to_path_buf());
    }
    None
}

fn get_dir_content(dir: &Path) -> anyhow::Result<Vec<DirEntry>> {
    let read_dir = fs::read_dir(Path::new(&dir))
        .with_context(|| format!("Failed to load {} directory", env!("CARGO_PKG_NAME")))?;
    Ok(read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect())
}

pub fn filter_installed_monotrail(
    specs: &[RequestedSpec],
    monotrail_root: &Path,
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    // Behold my monstrous iterator
    // name -> version -> compatible tag
    let installed_packages: Vec<(String, String, String)> = get_dir_content(monotrail_root)
        // No monotrail dir, no packages
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

/// script can be a manually set working directory or the python script we're running.
/// Returns a list name, python version, unique version
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn install_requested(
    specs: &[RequestedSpec],
    python: &Path,
    python_version: (u8, u8),
) -> anyhow::Result<(String, Vec<InstalledPackage>)> {
    let monotrail_root = monotrail_root()?;
    let compatible_tags = compatible_tags(python_version, &Os::current()?, &Arch::current()?)?;

    let (to_install_specs, installed_done) =
        filter_installed_monotrail(specs, Path::new(&monotrail_root))?;

    let mut installed = install_specs(
        &to_install_specs,
        &InstallLocation::Monotrail {
            monotrail_root: PathBuf::from(&monotrail_root),
            python: python.to_path_buf(),
            python_version,
        },
        &compatible_tags,
        false,
        true,
    )?;

    installed.extend(installed_done);

    let monotrail_location_string = monotrail_root
        .to_str()
        .with_context(|| format!("{} path is cursed", env!("CARGO_PKG_NAME")))?
        .to_string();
    debug!("python extension has {} packages", installed.len());
    Ok((monotrail_location_string, installed))
}

/// When python installs packages, it just unpacks zips into the venv. If multiples packages
/// contain the same directory, they are simply silently merged, and files are overwritten.
/// This means that packages can ship modules of a different nam, e.g. pillow containing PIL,
/// and one package silently extending another package. The latter is the case for poetry: The
/// "poetry" package depends on "poetry-core". "poetry-core" contains the `poetry/core/` submodule
/// and nothing else, while the "poetry" package contains all other submodules, such as
/// `poetry/io/` and `poetry/config/`. Both contain the same `poetry/__init__.py`. We install each
/// package in a different directory so that suddenly there's two dirs in our path finder that
/// contain `poetry/__init__.py` with separate parts of poetry. Luckily `ModuleSpec`, the thing
/// we're from our `PathFinder`, has a `submodule_search_locations` where we can find both
/// locations. This functions finds those locations be scanning all installed packages for which
/// modules they contain.
///
/// https://docs.python.org/3/library/importlib.html#importlib.machinery.ModuleSpec
///
/// Returns the name, the main file to import for the spec and the submodule_search_locations
/// as well as a list of .pth files that need to be executed
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn spec_paths(
    sprawl_root: &Path,
    sprawl_packages: &[InstalledPackage],
    python_version: (u8, u8),
) -> anyhow::Result<(HashMap<String, (PathBuf, Vec<PathBuf>)>, Vec<PathBuf>)> {
    let mut dir_modules: HashMap<String, Vec<InstalledPackage>> = HashMap::new();
    let mut file_modules: HashMap<String, (InstalledPackage, PathBuf)> = HashMap::new();
    let mut pth_files: Vec<PathBuf> = Vec::new();
    // https://peps.python.org/pep-0420/#specification
    for sprawl_package in sprawl_packages {
        let package_dir =
            sprawl_package.monotrail_site_packages(sprawl_root.to_path_buf(), python_version);
        let dir_contents =
            fs::read_dir(&package_dir)?.collect::<Result<Vec<DirEntry>, io::Error>>()?;
        // "If <directory>/foo/__init__.py is found, a regular package is imported and returned."
        for entry in dir_contents {
            let filename = if let Some(filename) = entry.file_name().to_str() {
                filename.to_string()
            } else {
                warn!("non-utf8 filename encountered in {}", package_dir.display());
                continue;
            };
            if entry.file_type()?.is_dir() && entry.path().join("__init__.py").is_file() {
                dir_modules
                    .entry(filename.to_string())
                    .or_default()
                    .push(sprawl_package.clone())
            }

            // "If not, but <directory>/foo.{py,pyc,so,pyd} is found, a module is imported and returned."
            // Can also be foo.<tag>.so
            if entry.file_type()?.is_file() {
                let parts: Vec<&str> = filename.split('.').collect();
                match *parts.as_slice() {
                    [stem, "py" | "pyc" | "so" | "pyd"] => {
                        file_modules
                            .insert(stem.to_string(), (sprawl_package.clone(), entry.path()));
                    }
                    [stem, _tag, "so"] => {
                        // TODO: Check compatibility of so tag
                        file_modules
                            .insert(stem.to_string(), (sprawl_package.clone(), entry.path()));
                    }
                    [.., "pth"] => pth_files.push(entry.path()),
                    _ => continue,
                }
            }
        }
    }

    // Make import order deterministic
    for value in dir_modules.values_mut() {
        value.sort_by_key(|package| package.name.clone());
    }

    let mut spec_bases: HashMap<String, (PathBuf, Vec<PathBuf>)> = HashMap::new();

    // Merge single file modules in while performing conflict detection
    for (name, (_single_file_packages, filename)) in file_modules {
        if dir_modules.contains_key(&name) {
            // This is the case e.g. for inflection 0.5.1
            continue;
        }

        spec_bases.insert(name, (filename, Vec::new()));
    }

    for (name, packages) in dir_modules {
        let submodule_search_locations = packages
            .iter()
            .map(|package| {
                package
                    .monotrail_site_packages(sprawl_root.to_path_buf(), python_version)
                    .join(&name)
            })
            .collect();
        // This is effectively a random pick, if someone is relying on different __init__.py
        // contents all is already cursed anyway.
        // TODO: Should we check __init__.py contents that they're all equal?
        let first_init_py = packages[0]
            .monotrail_site_packages(sprawl_root.to_path_buf(), python_version)
            .join(&name)
            .join("__init__.py");
        spec_bases.insert(name, (first_init_py, submodule_search_locations));
    }

    Ok((spec_bases, pth_files))
}

/// Goes up the script path until a pyproject.toml/poetry.lock or a requirements.txt is
/// found, for requirements.txt calls poetry to resolve the dependencies, reads the resolved
/// set and returns it. `script` can be a file or a directory or will default to the current
/// working directory
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn get_specs(
    script: Option<&Path>,
    extras: &[String],
    sys_executable: &Path,
    python_version: (u8, u8),
    pep508_env: &Pep508Environment,
) -> anyhow::Result<Vec<RequestedSpec>> {
    let dir_running = match script {
        None => current_dir().context("Couldn't get current directory ಠ_ಠ")?,
        Some(file) if file.is_file() => {
            if let Some(parent) = file.parent() {
                parent.to_path_buf()
            } else {
                bail!("File has no parent directory ಠ_ಠ: {}", file.display())
            }
        }
        Some(dir) if dir.is_dir() => dir.to_path_buf(),
        Some(neither) => {
            bail!(
                "Running file is neither file not directory (is the python invocation unsupported?): {}",
                neither.display()
            )
        }
    };
    debug!("python project dir: {}", dir_running.display());

    let (dep_file_location, lockfile_type) = find_dep_file(&dir_running).with_context(|| {
        format!(
            "pyproject.toml not found next to {} nor in any parent directory",
            script.map_or_else(
                || "current directory".to_string(),
                |file_running| file_running.display().to_string()
            )
        )
    })?;
    let specs = match lockfile_type {
        LockfileType::PyprojectToml => {
            let (poetry_toml, poetry_lock) = read_toml_files(&dep_file_location)?;
            read_poetry_specs(poetry_toml, poetry_lock, false, extras, pep508_env)?
        }
        LockfileType::RequirementsTxt => {
            let requirements_txt = fs::read_to_string(&dep_file_location)?;

            let requirements = parse_requirements_txt(&requirements_txt).map_err(|err| {
                anyhow::Error::msg(err).context(format!(
                    "requirements specification is invalid: {}",
                    dep_file_location.display()
                ))
            })?;
            let requirements = requirements
                .into_iter()
                .map(|(name, version)| {
                    (
                        name,
                        poetry_toml::Dependency::Compact(
                            // If no version is given, we'll let poetry pick one with `*`
                            version.as_deref().unwrap_or("*").to_string(),
                        ),
                    )
                })
                .collect();
            // We don't know whether the requirements.txt is from `pip freeze` or just a list of
            // version, so we let it go through poetry resolve either way. For a frozen file
            // there will just be no change
            let (poetry_toml, poetry_lock, _) = poetry_resolve(
                requirements,
                sys_executable,
                python_version,
                None,
                pep508_env,
            )?;
            read_poetry_specs(poetry_toml, poetry_lock, false, extras, pep508_env)?
        }
    };
    Ok(specs)
}
