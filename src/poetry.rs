//! Parsing of pyproject.toml and poetry.lock

use crate::markers::Pep508Environment;
use crate::poetry::poetry_lock::PoetryLock;
use crate::spec::{DistributionType, Spec};
use crate::wheel_tags::WheelFilename;
use anyhow::{anyhow, bail, Context};
use fs_err as fs;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::str::FromStr;

mod poetry_lock {
    use crate::markers::{parse_markers, Pep508Environment};
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct PoetryLock {
        pub package: Vec<Package>,
        pub metadata: Metadata,
    }

    /// `[[package]]`
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct Package {
        pub name: String,
        pub version: String,
        pub description: String,
        pub category: String,
        pub optional: bool,
        pub python_versions: String,
        #[serde(default)]
        pub extras: HashMap<String, Vec<String>>,
        pub dependencies: Option<HashMap<String, Dependency>>,
    }

    /// e.g. `{version = ">=1.21.0", markers = "python_version >= \"3.10\""}`
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct DependencyExpanded {
        pub version: String,
        pub markers: Option<String>,
        pub extras: Option<Vec<String>>,
    }

    /// `[package.dependencies]`
    ///
    /// Can be one of three formats:
    /// ```toml
    /// attrs = ">=17.4.0"
    /// colorama = {version = "*", markers = "sys_platform == \"win32\""}
    /// numpy = [
    ///     {version = ">=1.18.5", markers = "platform_machine != \"aarch64\" and platform_machine != \"arm64\" and python_version < \"3.10\""},
    ///     {version = ">=1.19.2", markers = "platform_machine == \"aarch64\" and python_version < \"3.10\""},
    ///     {version = ">=1.20.0", markers = "platform_machine == \"arm64\" and python_version < \"3.10\""},
    ///     {version = ">=1.21.0", markers = "python_version >= \"3.10\""},
    /// ]
    /// ```
    #[derive(Deserialize, Debug, Clone)]
    #[serde(untagged, rename_all = "kebab-case")]
    pub enum Dependency {
        Compact(String),
        Expanded(DependencyExpanded),
        List(Vec<DependencyExpanded>),
    }

    impl Dependency {
        /// checks if we need to install given the markers and returns the matching version constraint
        pub fn get_version(&self, environment: &Pep508Environment) -> Result<Option<&str>, String> {
            Ok(match self {
                Dependency::Compact(version) => Some(version),
                Dependency::Expanded(DependencyExpanded {
                    version, markers, ..
                }) => {
                    if let Some(markers) = markers {
                        if markers.starts_with("extra ==") {
                            todo!("markers extra");
                        }
                        if parse_markers(markers).unwrap().evaluate(environment)? {
                            return Ok(Some(version));
                        }
                    }
                    None
                }
                Dependency::List(options) => {
                    for option in options {
                        if let Some(markers) = &option.markers {
                            if markers.starts_with("extra ==") {
                                todo!("markers extra");
                            }
                            if parse_markers(markers).unwrap().evaluate(environment)? {
                                return Ok(Some(&option.version));
                            }
                        } else {
                            return Ok(Some(&option.version));
                        }
                    }
                    None
                }
            })
        }
    }

    /// `[metadata]`
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct Metadata {
        pub lock_version: String,
        pub python_versions: String,
        pub content_hash: String,
        /// `[metadata.files]`
        pub files: HashMap<String, Vec<HashedFile>>,
    }

    /// e.g. `{file = "attrs-21.4.0-py2.py3-none-any.whl", hash = "sha256:2d27e3784d7a565d36ab851fe94887c5eccd6a463168875832a1be79c82828b4"}`
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct HashedFile {
        pub file: String,
        pub hash: String,
    }
}

mod poetry_toml {
    use serde::Deserialize;
    use std::collections::HashMap;

    /// ```toml
    /// [tool]
    /// ```
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct PoetryPyprojectToml {
        pub tool: ToolSection,
    }

    /// ```toml
    /// [tool.poetry]
    /// ```
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct ToolSection {
        pub poetry: PoetrySection,
    }

    /// ```toml
    /// [tool.poetry.dependencies]
    /// dep1 = "1.2.3"
    /// dep2 = { version = "4.5.6", optional = true }
    /// ```
    #[derive(Deserialize, Debug, Clone)]
    #[serde(untagged, rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub enum Dependency {
        Compact(String),
        Expanded {
            version: String,
            optional: Option<bool>,
            extras: Option<Vec<String>>,
        },
    }

    impl Dependency {
        pub fn is_optional(&self) -> bool {
            match self {
                Dependency::Compact(_) => false,
                Dependency::Expanded { optional, .. } => optional.unwrap_or(false),
            }
        }
    }

    /// ```toml
    /// [tool.poetry.dependencies]
    /// [tool.poetry.dev-dependencies]
    /// [tool.poetry.extras]
    /// ``
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct PoetrySection {
        pub dependencies: HashMap<String, Dependency>,
        pub dev_dependencies: HashMap<String, Dependency>,
        pub extras: HashMap<String, Vec<String>>,
    }
}

/// Resolves a single package's filename and url inside a poetry lockfile
pub fn filename_and_url(
    lockfile: &PoetryLock,
    package: &poetry_lock::Package,
    compatible_tags: &[(String, String, String)],
) -> anyhow::Result<(String, DistributionType, String)> {
    let hashed_files = lockfile
        .metadata
        .files
        .get(&package.name)
        .context("invalid lockfile (missing file hashes), run `poetry update`")?;
    let filenames: Vec<_> = hashed_files
        .iter()
        .filter(|hashed_file| hashed_file.file.ends_with(".whl"))
        .map(|hashed_file| {
            Ok((
                hashed_file.file.clone(),
                WheelFilename::from_str(&hashed_file.file).with_context(|| {
                    format!(
                        "Couldn't parse wheel filename {} in lockfile",
                        hashed_file.file
                    )
                })?,
            ))
        })
        .collect::<Result<_, anyhow::Error>>()?;
    let wheel = filenames
        .iter()
        .find(|(_filename, parsed)| parsed.is_compatible(compatible_tags));

    if let Some((filename, parsed_filename)) = wheel {
        // https://warehouse.pypa.io/api-reference/integration-guide.html#if-you-so-choose
        let url = format!(
            "https://files.pythonhosted.org/packages/{}/{}/{}/{}",
            parsed_filename.python_tag.join("."),
            package.name.chars().next().unwrap(),
            package.name,
            filename,
        );
        return Ok((filename.clone(), DistributionType::Wheel, url));
    }

    if let Some(hashed_file) = hashed_files
        .iter()
        .find(|hashed_file| hashed_file.file.ends_with(".tar.gz"))
    {
        // https://warehouse.pypa.io/api-reference/integration-guide.html#if-you-so-choose
        let url = format!(
            "https://files.pythonhosted.org/packages/{}/{}/{}/{}",
            "source",
            package.name.chars().next().unwrap(),
            package.name,
            hashed_file.file,
        );
        Ok((
            hashed_file.file.clone(),
            DistributionType::SourceDistribution,
            url,
        ))
    } else {
        bail!(
            "No compatible compiled file found for {}. \
                Why does it have neither a wheel for your operating system/architecture/python version not any sdist?",
            package.name
        )
    }
}

/// Parses pyproject.toml and poetry.lock and returns a list of packages to install
pub fn find_specs_to_install(
    pyproject_toml: &Path,
    compatible_tags: &[(String, String, String)],
    no_dev: bool,
    extras: &[String],
    pep508_env: Option<Pep508Environment>,
) -> anyhow::Result<Vec<Spec>> {
    // TODO: don't parse this from subprocess but do it like maturin
    let environment = pep508_env.unwrap_or_else(Pep508Environment::from_python);

    // get deps from poetry.toml
    let poetry_pyproject_toml: poetry_toml::PoetryPyprojectToml =
        toml::from_str(&fs::read_to_string(pyproject_toml)?).with_context(|| {
            format!(
                "Invalid poetry pyproject.toml: {}",
                pyproject_toml.display()
            )
        })?;
    let poetry_section = poetry_pyproject_toml.tool.poetry;

    let deps = if no_dev {
        poetry_section.dependencies.clone()
    } else {
        poetry_section
            .dependencies
            .into_iter()
            .chain(poetry_section.dev_dependencies)
            .collect()
    };

    // read lockfile
    let lockfile = pyproject_toml
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("poetry.lock");

    let mut optionals_picked: HashSet<_> = HashSet::new();

    for extra_name in extras {
        let packages = poetry_section
            .extras
            .get(extra_name)
            .with_context(|| format!("No such extra {}", extra_name))?;
        optionals_picked.extend(packages);
    }

    let lockfile: poetry_lock::PoetryLock = toml::from_str(&fs::read_to_string(&lockfile)?)
        .with_context(|| format!("Invalid lockfile: {}", lockfile.display()))?;
    // keys are normalized names since `[package.dependencies]` also uses normalized names
    let packages: HashMap<String, &poetry_lock::Package> = lockfile
        .package
        .iter()
        .map(|package| (package.name.replace('-', "_"), package))
        .collect();

    let mut queue: VecDeque<String> = VecDeque::new();
    for (dep_name, dep_spec) in deps {
        if !dep_spec.is_optional() || optionals_picked.contains(&dep_name) {
            queue.push_back(dep_name);
        }
    }

    let mut seen = HashSet::new();
    let mut specs = Vec::new();

    while let Some(item) = queue.pop_front() {
        // We do not need to install python
        if item == "python" {
            continue;
        }
        // 1. Add package to install list
        // search by normalized name
        let package = packages
            .get(&item.to_lowercase().replace('-', "_"))
            .with_context(|| {
                format!(
                    "Lockfile outdated (run `poetry update`): {} is missing",
                    item
                )
            })?;
        let (filename, distribution_type, url) =
            filename_and_url(&lockfile, package, compatible_tags)?;
        let spec = Spec {
            requested: format!("{} {}", package.name, package.version),
            name: package.name.clone(),
            version: Some(package.version.clone()),
            file_path: None,
            url: Some((url, filename, distribution_type)),
        };
        specs.push(spec);

        // 2. Add package's deps to queue (basically flattened recursion)
        for (dep_item, dependency) in package.dependencies.as_ref().unwrap_or(&HashMap::new()) {
            if seen.contains(dep_item) {
                continue;
            }
            if let Some(_version) = dependency.get_version(&environment).map_err(|err| {
                anyhow!(err).context(format!(
                    "Failed to parse dependency {} of {}: {:?}",
                    dep_item, item, dependency,
                ))
            })? {
                queue.push_back(dep_item.clone());
            }

            seen.insert(dep_item.clone());
        }
    }

    Ok(specs)
}
