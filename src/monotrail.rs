use crate::install::InstalledPackage;
use crate::markers::Pep508Environment;
use crate::package_index::cache_dir;
use crate::poetry_integration::lock::poetry_resolve;
use crate::poetry_integration::poetry_toml;
use crate::poetry_integration::read_dependencies::poetry_spec_from_dir;
use crate::requirements_txt::parse_requirements_txt;
use crate::spec::RequestedSpec;
use crate::{install_specs, read_poetry_specs};
use anyhow::{bail, Context};
use fs_err as fs;
use fs_err::{DirEntry, File};
use install_wheel_rs::{compatible_tags, Arch, InstallLocation, Os};
use serde::Serialize;
use std::collections::HashMap;
use std::env::current_dir;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{env, io};
use tracing::{debug, warn};

enum LockfileType {
    PyprojectToml,
    RequirementsTxt,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LaunchType {
    /// We're coming from python, i.e. we're having pyo3 to run things
    PythonBindings,
    /// We're coming from our own binary entrypoint, i.e. we use libpython.so to run things
    Binary,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PythonContext {
    pub sys_executable: PathBuf,
    pub python_version: (u8, u8),
    pub pep508_env: Pep508Environment,
    pub launch_type: LaunchType,
}

/// The packaging and import data that is resolved by the rust part and deployed by the finder
#[cfg(not(feature = "python_bindings"))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FinderData {
    /// The location where all packages are installed
    pub sprawl_root: String,
    /// All resolved and installed packages indexed by name
    pub sprawl_packages: Vec<InstalledPackage>,
    /// Given a module name, where's the corresponding module file and what are the submodule_search_locations?
    pub spec_paths: HashMap<String, (PathBuf, Vec<PathBuf>)>,
    /// In from git mode where we check out a repository and make it available for import as if it was added to sys.path
    pub repo_dir: Option<PathBuf>,
    /// We need to run .pth files because some project such as matplotlib 3.5.1 use them to commit packaging crimes
    pub pth_files: Vec<PathBuf>,
    /// The contents of the last poetry.lock, used a basis for the next resolution when requirements
    /// change at runtime, both for faster resolution and in hopes the exact version stay the same
    /// so the user doesn't need to reload python
    pub lockfile: String,
    /// The installed scripts indexed by name. They are in the bin folder of each project, coming
    /// from entry_points.txt or data folder scripts
    pub scripts: HashMap<String, String>,
}

/// The packaging and import data that is resolved by the rust part and deployed by the finder
///
/// TODO: write a pyo3 bug report to parse through cfg attr
#[cfg(feature = "python_bindings")]
#[pyo3::pyclass(dict)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FinderData {
    #[pyo3(get)]
    pub sprawl_root: String,
    #[pyo3(get)]
    pub sprawl_packages: Vec<InstalledPackage>,
    #[pyo3(get)]
    pub spec_paths: HashMap<String, (PathBuf, Vec<PathBuf>)>,
    #[pyo3(get)]
    pub repo_dir: Option<PathBuf>,
    #[pyo3(get)]
    pub pth_files: Vec<PathBuf>,
    #[pyo3(get)]
    pub lockfile: String,
    #[pyo3(get)]
    pub scripts: HashMap<String, String>,
}

#[cfg_attr(feature = "python_bindings", pyo3::pymethods)]
impl FinderData {
    /// For debugging
    #[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
    fn to_json(&self) -> String {
        serde_json::to_string(&self).expect("Couldn't convert to json")
    }
}

pub fn monotrail_root() -> anyhow::Result<PathBuf> {
    if let Some(env_root) = env::var_os("MONOTRAIL_ROOT") {
        Ok(PathBuf::from(env_root))
    } else {
        Ok(cache_dir()?.join("installed"))
    }
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

/// Returns all subdirs in a directory
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
    compatible_tags: &[(String, String, String)],
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    // Behold my monstrous iterator
    // name -> version -> compatible tag
    let mut compatible: Vec<(String, String, String)> = Vec::new();
    // No monotrail dir, no packages
    for name_dir in get_dir_content(monotrail_root).unwrap_or_default() {
        for version_dir in get_dir_content(&name_dir.path())? {
            for tag_dir in get_dir_content(&version_dir.path())? {
                let tag = tag_dir.file_name().to_string_lossy().to_string();
                let is_compatible = match tag.split('-').collect::<Vec<_>>()[..] {
                    [python_tag, abi_tag, platform_tag] => compatible_tags.iter().any(|ok_tag| {
                        python_tag.contains(&ok_tag.0)
                            && abi_tag.contains(&ok_tag.1)
                            && platform_tag.contains(&ok_tag.2)
                    }),
                    _ => {
                        warn!(
                            "Invalid tag {} in {}, skipping",
                            tag,
                            tag_dir.path().display()
                        );
                        continue;
                    }
                };
                if !is_compatible {
                    continue;
                }
                compatible.push((
                    name_dir.file_name().to_string_lossy().to_string(),
                    version_dir.file_name().to_string_lossy().to_string(),
                    tag,
                ))
            }
        }
    }

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
                compatible.iter().find(|(name, installed_version, _tag)| {
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
            if let Some((name, unique_version, _path)) = compatible
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
        filter_installed_monotrail(specs, Path::new(&monotrail_root), &compatible_tags)?;

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
/// working directory.
///
/// Returns the specs and the entrypoints of the root package (if poetry, empty for
/// requirements.txt)
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn get_specs(
    script: Option<&Path>,
    extras: &[String],
    python_context: &PythonContext,
) -> anyhow::Result<(Vec<RequestedSpec>, HashMap<String, String>, String)> {
    let dir_running = match script {
        None => current_dir().context("Couldn't get current directory ಠ_ಠ")?,
        Some(file) if file.is_file() => {
            let path = if let Some(parent) = file.parent() {
                parent.to_path_buf()
            } else {
                bail!("File has no parent directory ಠ_ಠ: {}", file.display())
            };
            let grandma = path.parent().unwrap_or_else(|| Path::new("/dev/null"));
            let root_marker = grandma.join(format!("{}-root-marker.txt", env!("CARGO_PKG_NAME")));

            if root_marker.is_file() {
                // This is the system created in `scripts_to_path` to communicate through execve
                PathBuf::from(
                    fs::read_to_string(root_marker).context("Failed to read root marger")?,
                )
            } else {
                path
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
    match lockfile_type {
        LockfileType::PyprojectToml => {
            poetry_spec_from_dir(&dep_file_location, extras, &python_context.pep508_env)
        }
        LockfileType::RequirementsTxt => {
            let (specs, lockfile) = specs_from_requirements_txt_resolved(
                &dep_file_location,
                extras,
                None,
                python_context,
            )?;
            Ok((specs, HashMap::new(), lockfile))
        }
    }
}

/// Reads the requirements.txt, calls poetry to resolve them and returns the resolved specs and the
/// lockfile
pub fn specs_from_requirements_txt_resolved(
    requirements_txt: &Path,
    extras: &[String],
    lockfile: Option<&str>,
    python_context: &PythonContext,
) -> anyhow::Result<(Vec<RequestedSpec>, String)> {
    let requirements = fs::read_to_string(&requirements_txt)?;

    let requirements = parse_requirements_txt(&requirements).map_err(|err| {
        anyhow::Error::msg(err).context(format!(
            "requirements specification is invalid: {}",
            requirements_txt.display()
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
    let (poetry_toml, poetry_lock, lockfile) =
        poetry_resolve(requirements, lockfile, python_context)
            .context("Failed to resolve dependencies with poetry")?;
    let specs = read_poetry_specs(
        poetry_toml,
        poetry_lock,
        false,
        extras,
        &python_context.pep508_env,
    )?;
    Ok((specs, lockfile))
}

/// Convenience wrapper around `install_requested` and `spec_paths`
pub fn install_specs_to_finder(
    specs: &[RequestedSpec],
    scripts: HashMap<String, String>,
    lockfile: String,
    repo_dir: Option<PathBuf>,
    python_context: &PythonContext,
) -> anyhow::Result<FinderData> {
    let (sprawl_root, sprawl_packages) = install_requested(
        specs,
        &python_context.sys_executable,
        python_context.python_version,
    )?;
    let (spec_paths, pth_files) = spec_paths(
        sprawl_root.as_ref(),
        &sprawl_packages,
        python_context.python_version,
    )?;

    // ugly hack: jupyter otherwise tries to locate its kernel.json relative to the python
    // interpreter, while we're installing them relative to the jupyter package.
    // If you want to help this project please make a pull request to jupyter to also make it search
    // relative to the package, based on ipykernel.__file__ or ipykernel.__path__ :)
    // https://docs.jupyter.org/en/latest/use/jupyter-directories.html#data-files
    if let Some(jupyter) = sprawl_packages.iter().find(|x| x.name == "ipykernel") {
        let mut jupyter_path = jupyter
            .monotrail_location(PathBuf::from(&sprawl_root))
            .join("share")
            .join("jupyter")
            .into_os_string();
        if let Some(existing_jupyter_path) = env::var_os("JUPYTER_PATH") {
            jupyter_path.push(":");
            jupyter_path.push(existing_jupyter_path);
        }
        env::set_var("JUPYTER_PATH", jupyter_path);
    }

    let finder_data = FinderData {
        sprawl_root,
        sprawl_packages,
        spec_paths,
        repo_dir,
        pth_files,
        lockfile,
        scripts,
    };

    Ok(finder_data)
}

/// In a venv, we would have all scripts collected into .venv/bin/ (on linux and mac). Here,
/// we not to collect them ourselves
pub fn find_scripts(
    packages: &[InstalledPackage],
    sprawl_root: &Path,
) -> anyhow::Result<HashMap<String, PathBuf>> {
    let mut scripts = HashMap::new();
    for package in packages {
        let bin_dir = package
            .monotrail_location(sprawl_root.to_path_buf())
            .join("bin");
        if !bin_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&bin_dir)? {
            let entry = entry?;
            if !entry.metadata()?.is_file() {
                continue;
            }

            scripts.insert(
                entry.file_name().to_string_lossy().to_string(),
                entry.path(),
            );
        }
    }
    Ok(scripts)
}

pub fn is_python_script(executable: &Path) -> anyhow::Result<bool> {
    // Check whether we're launching a monotrail python script
    let mut executable_file = File::open(&executable)
        .context("the executable file was right there and is now unreadable ಠ_ಠ")?;
    let placeholder_python = b"#!python";
    // scripts might be binaries, so we read an exact number of bytes instead of the first line as string
    let mut start = Vec::new();
    start.resize(placeholder_python.len(), 0);
    executable_file.read_exact(&mut start)?;
    let is_python_script = start == placeholder_python;
    Ok(is_python_script)
}
