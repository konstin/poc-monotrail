use crate::inject_and_run::{
    inject_and_run_python, prepare_execve_environment, run_python_args_finder_data,
};
use crate::install::{install_all, InstalledPackage};
use crate::markers::marker_environment_from_python;
use crate::poetry_integration::lock::poetry_resolve;
use crate::poetry_integration::read_dependencies::{
    poetry_spec_from_dir, read_requirements_for_poetry, specs_from_git,
};
use crate::spec::RequestedSpec;
use crate::utils::{cache_dir, get_dir_content};
use crate::{read_poetry_specs, DEFAULT_PYTHON_VERSION};
use anyhow::{bail, Context};
use fs_err as fs;
use fs_err::{DirEntry, File};
use install_wheel_rs::{CompatibleTags, InstallLocation, Script, SHEBANG_PYTHON};
use monotrail_utils::parse_cpython_args::determine_python_version;
use monotrail_utils::standalone_python::provision_python;
use pep508_rs::MarkerEnvironment;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::env::{current_dir, current_exe};
#[cfg(unix)]
use std::ffi::CString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, io};
use tempfile::TempDir;
use tracing::{debug, info, trace, warn};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum LockfileType {
    /// poetry.lock, which means a pyproject.toml also needs to exist
    PoetryLock,
    /// pyproject.toml, we assume it's one with poetry config
    PyprojectToml,
    /// requirements.txt, we parse a subset of it
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
    pub version: (u8, u8),
    pub pep508_env: MarkerEnvironment,
    pub launch_type: LaunchType,
}

/// Name of the import -> (`__init__.py`, submodule import dirs)
pub type SpecPaths = BTreeMap<String, (Option<PathBuf>, Vec<PathBuf>)>;

/// The [FinderData] is made by the installation system, the other fields are made by the inject
/// system
#[cfg_attr(feature = "python_bindings", pyo3::pyclass(dict, get_all))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InjectData {
    /// The location of packages and imports
    pub finder_data: FinderData,
    /// For some reason on windows the location of the monotrail containing folder gets
    /// inserted into `sys.path` so we need to remove it manually
    pub sys_path_removes: Vec<String>,
    /// Windows for some reason ignores `Py_SetProgramName`, so we need to set `sys.executable`
    /// manually
    pub sys_executable: String,
}

/// The packaging and import data that is resolved by the rust part and deployed by the finder
///
/// Keep in sync with its python counterparts in convert_finder_data.py and monotrail.pyi
#[cfg_attr(feature = "python_bindings", pyo3::pyclass(dict, get_all))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FinderData {
    /// The location where all packages are installed
    pub sprawl_root: String,
    /// All resolved and installed packages indexed by name
    pub sprawl_packages: Vec<InstalledPackage>,
    /// Given a module name, where's the corresponding module file and what are the submodule_search_locations?
    pub spec_paths: SpecPaths,
    /// In from git mode where we check out a repository and make it available for import as if it was added to sys.path
    pub project_dir: Option<PathBuf>,
    /// We need to run .pth files because some project such as matplotlib 3.5.1 use them to commit packaging crimes
    pub pth_files: Vec<PathBuf>,
    /// The contents of the last poetry.lock, used a basis for the next resolution when requirements
    /// change at runtime, both for faster resolution and in hopes the exact version stay the same
    /// so the user doesn't need to reload python
    pub lockfile: String,
    /// The scripts in pyproject.toml
    pub root_scripts: BTreeMap<String, Script>,
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
    // TODO: Make an cli arg everywhere to set this
    if let Some(env_root) = env::var_os(format!("{}_ROOT", env!("CARGO_PKG_NAME").to_uppercase())) {
        Ok(PathBuf::from(env_root))
    } else {
        Ok(cache_dir()?.join("installed"))
    }
}

/// Walks the directory tree up to find a pyproject.toml or a requirements.txt and returns
/// the dir (poetry) or the file (requirements.txt)
fn find_dep_file(dir_running: &Path) -> Option<(PathBuf, LockfileType)> {
    for ancestor in dir_running.ancestors() {
        if ancestor.join("poetry.lock").exists() {
            return Some((ancestor.to_path_buf(), LockfileType::PoetryLock));
        } else if ancestor.join("pyproject.toml").exists() {
            return Some((ancestor.to_path_buf(), LockfileType::PyprojectToml));
        } else if ancestor.join("requirements.txt").exists() {
            return Some((
                ancestor.join("requirements.txt"),
                LockfileType::RequirementsTxt,
            ));
        }
    }
    None
}

/// Returns the list of installed packages, optionally filtering for compatible tags.
///
/// This filtering is only done here to avoid messing with split/unsplit tags later
pub fn list_installed(
    root: &Path,
    compatible_tags: Option<&CompatibleTags>,
) -> anyhow::Result<Vec<(String, String, String)>> {
    // Behold my monstrous iterator
    // name -> version -> compatible tag
    let mut compatible = Vec::new();
    // No monotrail dir, no packages
    for name_dir in get_dir_content(root).unwrap_or_default() {
        for version_dir in get_dir_content(&name_dir.path())? {
            for tag_dir in get_dir_content(&version_dir.path())? {
                let tag = tag_dir.file_name().to_string_lossy().to_string();
                let is_compatible = match tag.split('-').collect::<Vec<_>>()[..] {
                    [python_tag, abi_tag, platform_tag] => {
                        if let Some(compatible_tags) = compatible_tags {
                            compatible_tags.iter().any(|ok_tag| {
                                python_tag.contains(&ok_tag.0)
                                    && abi_tag.contains(&ok_tag.1)
                                    && platform_tag.contains(&ok_tag.2)
                            })
                        } else {
                            true
                        }
                    }
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
    Ok(compatible)
}

/// Splits the given spec set into installed and to-be-installed
pub fn filter_installed_monotrail(
    specs: &[RequestedSpec],
    monotrail_root: &Path,
    compatible_tags: &CompatibleTags,
) -> anyhow::Result<(Vec<RequestedSpec>, Vec<InstalledPackage>)> {
    let compatible = list_installed(&monotrail_root, Some(compatible_tags))
        .context("Failed to collect installed packages")?;
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
pub fn install_missing(
    specs: &[RequestedSpec],
    python: &Path,
    python_version: (u8, u8),
) -> anyhow::Result<(String, Vec<InstalledPackage>)> {
    let monotrail_root = monotrail_root()?;
    let compatible_tags = CompatibleTags::current(python_version)?;

    // Lock install directory to prevent races between multiple monotrail processes. We need to
    // lock before determining which packages to install because another process might install
    // packages meanwhile and then we'll clash later because the package is then already installed.
    // We also it here instead of install_wheel to allow multithreading, since we'll only install
    // disjoint packages
    let location = InstallLocation::Monotrail {
        monotrail_root: PathBuf::from(&monotrail_root),
        python: python.to_path_buf(),
        python_version,
    }
    .acquire_lock()?;

    let (to_install_specs, installed_done) =
        filter_installed_monotrail(specs, Path::new(&monotrail_root), &compatible_tags)?;

    let mut installed = install_all(
        &to_install_specs,
        &location,
        &compatible_tags,
        false,
        true,
        false,
    )?;

    installed.extend(installed_done);
    // Helps debugging
    installed.sort_by(|left, right| left.name.cmp(&right.name));

    let monotrail_location_string = monotrail_root
        .to_str()
        .with_context(|| format!("{} path is cursed", env!("CARGO_PKG_NAME")))?
        .to_string();
    debug!("Prepared {} packages", installed.len());
    trace!(
        "Installed Packages:{}",
        installed
            .iter()
            .map(|package| format!(
                "    {} {} {} {}",
                package.name, package.python_version, package.unique_version, package.tag
            ))
            .fold(String::new(), |acc, value| acc + "\n" + &value)
    );
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
/// <https://docs.python.org/3/library/importlib.html#importlib.machinery.ModuleSpec>
///
/// Returns the name, the main file to import for the spec and the submodule_search_locations
/// as well as a list of .pth files that need to be executed
#[cfg_attr(not(feature = "python_bindings"), allow(dead_code))]
pub fn spec_paths(
    sprawl_root: &Path,
    sprawl_packages: &[InstalledPackage],
    python_version: (u8, u8),
) -> anyhow::Result<(SpecPaths, Vec<PathBuf>)> {
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
            // namespace modules grml hlf
            if entry.file_type()?.is_dir() {
                // && entry.path().join("__init__.py").is_file() {
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
                    [stem, _tag, "so" | "pyd"] => {
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

    let mut spec_bases: SpecPaths = BTreeMap::new();

    // Merge single file modules in while performing conflict detection
    for (name, (_single_file_packages, filename)) in file_modules {
        if dir_modules.contains_key(&name) {
            // This is the case e.g. for inflection 0.5.1
            continue;
        }

        spec_bases.insert(name, (Some(filename), Vec::new()));
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
        // This is effectively a random pick (even though deterministic), if someone is relying
        // on different __init__.py contents all is already cursed anyway.
        // TODO: Should we check __init__.py contents that they're all equal?
        let first_init_py = packages
            .iter()
            .map(|package| {
                package
                    .monotrail_site_packages(sprawl_root.to_path_buf(), python_version)
                    .join(&name)
                    .join("__init__.py")
            })
            .find(|init_py| init_py.is_file());

        if let Some(first_init_py) = first_init_py {
            spec_bases.insert(
                name.clone(),
                (Some(first_init_py), submodule_search_locations),
            );
        } else {
            // If there's no __init__.py, we have a namespace module
            spec_bases.insert(name.clone(), (None, submodule_search_locations));
        }
    }

    Ok((spec_bases, pth_files))
}

/// Goes up the script path until a pyproject.toml/poetry.lock or a requirements.txt is
/// found, for requirements.txt calls poetry to resolve the dependencies, reads the resolved
/// set and returns it. `script` can be a file or a directory or will default to the current
/// working directory.
///
/// Returns the specs, the entrypoints of the root package (if poetry, empty for
/// requirements.txt), the lockfile and the root dir
#[allow(clippy::type_complexity)]
pub fn load_specs(
    script: Option<&Path>,
    extras: &[String],
    python_context: &PythonContext,
) -> anyhow::Result<(
    Vec<RequestedSpec>,
    BTreeMap<String, Script>,
    String,
    PathBuf,
)> {
    let project_dir = match script {
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
                    fs::read_to_string(root_marker).context("Failed to read root marker")?,
                )
            } else {
                path
            }
        }
        Some(dir) if dir.is_dir() => dir.to_path_buf(),
        Some(underscore) if underscore == Path::new("-") => {
            // stdin
            current_dir().context("Couldn't get current directory ಠ_ಠ")?
        }
        Some(neither) => {
            bail!(
                "Running file is neither file not directory (is the python invocation unsupported?): {}",
                neither.display()
            )
        }
    };
    debug!("python project dir: {}", project_dir.display());

    let (dep_file_location, lockfile_type) = find_dep_file(&project_dir).with_context(|| {
        format!(
            "neither pyproject.toml nor requirements.txt not found next to {} nor in any parent directory",
            script.map_or_else(
                || "current directory".to_string(),
                |file_running| file_running.display().to_string()
            )
        )
    })?;
    match lockfile_type {
        LockfileType::PoetryLock | LockfileType::PyprojectToml => {
            // If there's no poetry.lock yet, we need to call `poetry lock` first to create it
            if lockfile_type == LockfileType::PyprojectToml {
                info!(
                    "No poetry.lock found, running `{} poetry lock`",
                    env!("CARGO_PKG_NAME")
                );
                // Run in subprocess so as not to pollute the current process by already injecting
                // python
                let current_exe =
                    current_exe().context("Couldn't determine currently running program 🤨")?;
                let status = Command::new(&current_exe)
                    .args(["poetry", "lock"])
                    .status()
                    .with_context(|| {
                        format!("Failed to run `{} poetry lock`", current_exe.display())
                    })?;
                if !status.success() {
                    bail!(
                        "Failed to run `{} poetry lock`: {}",
                        current_exe.display(),
                        status
                    )
                }
            }
            let (specs, root_scripts, lockfile) =
                poetry_spec_from_dir(&dep_file_location, extras, &python_context.pep508_env)
                    .context("Couldn't load specs from pyproject.toml/poetry.lock")?;
            Ok((specs, root_scripts, lockfile, project_dir))
        }
        LockfileType::RequirementsTxt => {
            let (specs, lockfile) = specs_from_requirements_txt_resolved(
                &dep_file_location,
                extras,
                None,
                python_context,
            )?;
            Ok((specs, BTreeMap::new(), lockfile, project_dir))
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
    let requirements = read_requirements_for_poetry(&requirements_txt, &current_dir()?)?;
    // We don't know whether the requirements.txt is from `pip freeze` or just a list of
    // version, so we let it go through poetry resolve either way. For a frozen file
    // there will just be no change
    let (poetry_section, poetry_lock, lockfile) =
        poetry_resolve(&requirements, lockfile, python_context)
            .context("Failed to resolve dependencies with poetry")?;
    let specs = read_poetry_specs(
        &poetry_section,
        poetry_lock,
        false,
        extras,
        &python_context.pep508_env,
    )?;
    Ok((specs, lockfile))
}

/// Convenience wrapper around `install_requested` and `spec_paths`
pub fn install(
    specs: &[RequestedSpec],
    root_scripts: BTreeMap<String, Script>,
    lockfile: String,
    project_dir: Option<PathBuf>,
    python_context: &PythonContext,
) -> anyhow::Result<FinderData> {
    let (sprawl_root, sprawl_packages) = install_missing(
        specs,
        &python_context.sys_executable,
        python_context.version,
    )?;
    let (spec_paths, pth_files) = spec_paths(
        sprawl_root.as_ref(),
        &sprawl_packages,
        python_context.version,
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
            // With execve this might already be set
            if existing_jupyter_path != jupyter_path {
                jupyter_path.push(":");
                jupyter_path.push(existing_jupyter_path);
            }
        }
        debug!(
            "Detected ipykernel, setting JUPYTER_PATH to {}",
            jupyter_path.to_string_lossy()
        );
        env::set_var("JUPYTER_PATH", jupyter_path);
    }

    let finder_data = FinderData {
        sprawl_root,
        sprawl_packages,
        spec_paths,
        project_dir,
        pth_files,
        lockfile,
        root_scripts,
    };

    Ok(finder_data)
}

/// In a venv, we would have all scripts collected into .venv/bin/ (on linux and mac). Here,
/// we not to collect them ourselves
pub fn find_scripts(
    packages: &[InstalledPackage],
    sprawl_root: &Path,
) -> anyhow::Result<BTreeMap<String, PathBuf>> {
    let mut scripts = BTreeMap::new();
    for package in packages {
        let bin_dir = package
            .monotrail_location(sprawl_root.to_path_buf())
            .join(if cfg!(windows) { "Scripts" } else { "bin" });
        if !bin_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&bin_dir)? {
            let entry = entry?;
            if !entry.metadata()?.is_file() {
                continue;
            }

            let entry_name = if cfg!(windows) {
                // All the windows scripts we install end with .exe
                if let Some(stem) = entry.file_name().to_string_lossy().strip_suffix(".exe") {
                    stem.to_string()
                } else {
                    continue;
                }
            } else {
                // unix scripts are normal .py
                entry.file_name().to_string_lossy().to_string()
            };

            scripts.insert(entry_name, entry.path());
        }
    }
    trace!(
        "Found {} scripts: {:?}",
        scripts.keys().len(),
        scripts.keys().collect::<Vec<_>>()
    );
    Ok(scripts)
}

pub fn is_python_script(executable: &Path) -> anyhow::Result<bool> {
    // Check whether we're launching a monotrail python script
    let mut executable_file = File::open(executable)
        .context("the executable file was right there and is now unreadable ಠ_ಠ")?;
    // scripts might be binaries, so we read an exact number of bytes instead of the first line as string
    let mut start = vec![0; SHEBANG_PYTHON.as_bytes().len()];
    executable_file.read_exact(&mut start)?;
    let is_python_script = start == SHEBANG_PYTHON.as_bytes();
    Ok(is_python_script)
}

/// Run an installed command
pub fn run_command(
    extras: &[String],
    python_version: Option<&str>,
    root: Option<&Path>,
    command: &str,
    args: &[String],
) -> anyhow::Result<i32> {
    let (args, python_version) =
        determine_python_version(args, python_version, DEFAULT_PYTHON_VERSION)?;
    let (python_context, python_home) = provision_python_env(python_version)?;
    let (specs, root_scripts, lockfile, root) = load_specs(root, extras, &python_context)?;
    let finder_data = install(&specs, root_scripts, lockfile, Some(root), &python_context)?;

    // We need to resolve the command when we pass it to python so we need to remove the bare
    // command. TODO: Do we want to fake argv with the bare command? At least for code in tracebacks
    // we need to pass the real file to python
    let trail_args = args[1..].to_vec();
    run_command_finder_data(
        &command,
        &trail_args,
        &python_context,
        &python_home,
        &finder_data,
    )
}

pub fn run_command_finder_data(
    script: &str,
    args: &[String],
    python_context: &PythonContext,
    python_home: &Path,
    finder_data: &FinderData,
) -> anyhow::Result<i32> {
    let scripts = find_scripts(
        &finder_data.sprawl_packages,
        Path::new(&finder_data.sprawl_root),
    )
    .context("Failed to collect scripts")?;
    let scripts_tmp = TempDir::new().context("Failed to create tempdir")?;
    let (sys_executable, path_dir) = prepare_execve_environment(
        &scripts,
        &finder_data.root_scripts,
        finder_data.project_dir.as_deref(),
        scripts_tmp.path(),
        python_context.version,
    )?;

    // There's two possible script sources: pyproject.toml of the root or the collected scripts
    // of all dependencies
    let script_path = if let Some(script) = finder_data.root_scripts.get(script) {
        // prepare_execve_environment has created that wrapper script
        path_dir.join(&script.script_name)
    } else if let Some(script_path) = scripts.get(&script.to_string()) {
        script_path.clone()
    } else {
        let mut all_scripts: Vec<&str> = scripts
            .keys()
            .chain(finder_data.root_scripts.keys())
            .map(|x| x.as_str())
            .collect();
        all_scripts.sort_unstable();

        bail!(
            "Couldn't find command {} in installed packages. Installed scripts: {:?}",
            script,
            all_scripts.join(" ")
        )
    };
    // TODO: Properly handle windows here
    // Current logic: The binaries we produce can't actually launch for themselves due to some bug,
    // but running the binary through python works (?!) and python even displays it as running
    // `some_script.exe/__main__.py`.
    let exit_code = if is_python_script(&script_path)? || cfg!(windows) {
        let args: Vec<String> = [
            python_context.sys_executable.to_string_lossy().to_string(),
            script_path.to_string_lossy().to_string(),
        ]
        .iter()
        .chain(args)
        .map(ToString::to_string)
        .collect();
        debug!("launching (python) {:?}", args);

        inject_and_run_python(
            &python_home,
            python_context.version,
            &sys_executable,
            &args,
            &finder_data,
        )?
    } else {
        debug!("launching (execv) {}", script_path.display());
        #[cfg(unix)]
        {
            // Sorry for the to_string_lossy all over the place
            // https://stackoverflow.com/a/38948854/3549270
            // unwrap safety: Strings can never contain internal null bytes
            let executable_c_str = CString::new(script_path.to_string_lossy().as_bytes()).unwrap();
            let args_c_string = args
                .iter()
                .map(|arg| {
                    // unwrap safety: Strings can never contain internal null bytes
                    CString::new(arg.as_bytes()).unwrap()
                })
                .collect::<Vec<CString>>();

            // We replace the current process with the new process is it's like actually just running
            // the real thing.
            // Note the that this may launch a python script, a native binary or anything else
            // unwrap safety: Infallible (that's the actual type)
            nix::unistd::execv(&executable_c_str, &args_c_string).unwrap();
            unreachable!()
        }
        #[cfg(windows)]
        {
            // TODO: What's the correct equivalent of execv on windows? I couldn't find one, but
            // there should be one
            let status = Command::new(script_path)
                .args(args.iter())
                .status()
                .context("Failed to launch process")?;
            status
                .code()
                .context("Process didn't return an exit code")?
        }
        #[cfg(not(any(unix, windows)))]
        compile_error!("Unsupported Platform")
    };
    // just to assert it lives until here
    drop(scripts_tmp);
    Ok(exit_code)
}

/// Like `git pull <repo> <tmpdir> && cd <tmpdir> && git checkout <rev> && monotrail run <...>`,
/// mostly here to mirror the python `monotrail.from_git()` function
pub fn cli_from_git(
    git_url: &str,
    revision: &str,
    extras: &[String],
    python_version: Option<String>,
    args: &[String],
) -> anyhow::Result<Option<i32>> {
    let trail_args = args[1..].to_vec();
    let (trail_args, python_version) = determine_python_version(
        &trail_args,
        python_version.as_deref(),
        DEFAULT_PYTHON_VERSION,
    )?;
    let (python_context, python_home) = provision_python_env(python_version)?;

    let (specs, repo_dir, lockfile) =
        specs_from_git(git_url, revision, &extras, None, &python_context)?;

    let finder_data = install(
        &specs,
        BTreeMap::new(),
        lockfile,
        Some(repo_dir.clone()),
        &python_context,
    )?;

    let exit_code = match args[0].as_str() {
        "python" => run_python_args_finder_data(
            Some(&repo_dir),
            trail_args,
            &python_context,
            &python_home,
            &finder_data,
        )?,
        "command" => run_command_finder_data(
            // If there's no command this will show an error downstream
            &args.get(1).unwrap_or(&"".to_string()),
            &trail_args,
            &python_context,
            &python_home,
            &finder_data,
        )?,
        other => bail!("invalid command `{}`, must be 'python' or 'command'", other),
    };
    Ok(Some(exit_code))
}

/// If a downloaded python version exists, return this, otherwise download and unpack a matching one
/// from indygreg/python-build-standalone
pub fn provision_python_env(python_version: (u8, u8)) -> anyhow::Result<(PythonContext, PathBuf)> {
    let (python_binary, python_home) = provision_python(python_version, cache_dir()?.as_path())?;

    // TODO: Already init and use libpython here
    let pep508_env = marker_environment_from_python(&python_binary);
    let python_context = PythonContext {
        sys_executable: python_binary,
        version: python_version,
        pep508_env,
        launch_type: LaunchType::Binary,
    };

    Ok((python_context, python_home))
}
