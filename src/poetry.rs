//! Parsing of pyproject.toml and poetry.lock

use crate::markers::Pep508Environment;
use crate::poetry::poetry_lock::{Package, PoetryLock};
use crate::poetry::poetry_toml::Dependency;
use crate::spec::{DistributionType, Spec};
use crate::wheel_tags::WheelFilename;
use crate::WheelInstallerError;
use anyhow::{bail, Context};
use fs_err as fs;
use regex::Regex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::str::FromStr;

mod poetry_lock {
    use crate::markers::{parse_markers, Pep508Environment};
    use regex::Regex;
    use serde::Deserialize;
    use std::collections::{HashMap, HashSet};

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
        ///
        /// For the extras we give in a set of extras the is activated for self to check if we need
        /// self->dep, and return the extras active for self->dep
        pub fn get_version_and_extras(
            &self,
            environment: &Pep508Environment,
            self_extras: &HashSet<String>,
        ) -> Result<Option<(String, Vec<String>)>, String> {
            let extra_re = Regex::new(r#"^extra == "([\w\d_-]+)"$"#).unwrap();

            Ok(match self {
                Dependency::Compact(version) => Some((version.to_string(), Vec::new())),
                Dependency::Expanded(DependencyExpanded {
                    version,
                    markers,
                    extras,
                }) => {
                    if let Some(markers) = markers {
                        if let Some(captures) = extra_re.captures(markers) {
                            return if self_extras.contains(&captures[1].to_string()) {
                                Ok(Some((
                                    version.to_string(),
                                    extras.clone().unwrap_or_default(),
                                )))
                            } else {
                                Ok(None)
                            };
                        }
                        if parse_markers(markers).unwrap().evaluate(environment)? {
                            return Ok(Some((
                                version.to_string(),
                                extras.clone().unwrap_or_default(),
                            )));
                        }
                    }
                    None
                }
                Dependency::List(options) => {
                    for option in options {
                        if let Some(markers) = &option.markers {
                            if let Some(captures) = extra_re.captures(markers) {
                                if self_extras.contains(&captures[1].to_string()) {
                                    return Ok(Some((
                                        option.version.to_string(),
                                        option.extras.clone().unwrap_or_default(),
                                    )));
                                } else {
                                    continue;
                                };
                            }
                            if parse_markers(markers).unwrap().evaluate(environment)? {
                                return Ok(Some((
                                    option.version.to_string(),
                                    option.extras.clone().unwrap_or_default(),
                                )));
                            }
                        } else {
                            return Ok(Some((
                                option.version.to_string(),
                                option.extras.clone().unwrap_or_default(),
                            )));
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

        pub fn get_extras(&self) -> &[String] {
            match self {
                Dependency::Compact(_) => &[],
                Dependency::Expanded { extras, .. } => extras.as_deref().unwrap_or_default(),
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
///
/// doesn't work because the pypi api wants a different python version than the one in the wheel
/// filename
#[allow(dead_code)]
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

/// this isn't actually needed poetry gives us all we need
#[allow(dead_code)]
fn parse_dep_extra(
    dep_spec: &str,
) -> Result<(String, HashSet<String>, Option<String>), WheelInstallerError> {
    let re = Regex::new(r"(?P<name>[\w\d_-]+)(?:\[(?P<extras>.*)\])? ?(?:\((?P<version>.+)\))?")
        .unwrap();
    let captures = re.captures(dep_spec).ok_or_else(|| {
        WheelInstallerError::InvalidWheel(format!(
            "Invalid dependency specification in poetry.lock: {}",
            dep_spec
        ))
    })?;
    Ok((
        captures.name("name").unwrap().as_str().to_string(),
        captures
            .name("extras")
            .map(|extras| {
                extras
                    .as_str()
                    .to_string()
                    .split(',')
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        captures
            .name("version")
            .map(|version| version.as_str().to_string()),
    ))
}

fn resolution_to_specs(
    packages: HashMap<String, Package>,
    deps_with_extras: HashMap<String, HashSet<String>>,
) -> anyhow::Result<Vec<Spec>> {
    let mut specs = Vec::new();
    for (dep_name, dep_extras) in deps_with_extras {
        // search by normalized name
        let package = packages
            .get(&dep_name.to_lowercase().replace('-', "_"))
            .with_context(|| {
                format!(
                    "Lockfile outdated (run `poetry update`): {} is missing",
                    dep_name
                )
            })?;
        let spec = Spec {
            requested: format!("{} {}", package.name, package.version),
            name: package.name.clone(),
            version: Some(package.version.clone()),
            extras: dep_extras.into_iter().collect(),
            file_path: None,
            url: None,
        };
        specs.push(spec);
    }
    Ok(specs)
}

/// Get the root deps from pyproject.toml, already filtered by activated extras.
/// The is no root package in poetry.lock that we could use so we also need to read pyproject.toml
fn get_root_info(
    pyproject_toml: &&Path,
    no_dev: bool,
    extras: &[String],
) -> anyhow::Result<HashMap<String, Dependency>> {
    // read/get deps from poetry.toml
    let poetry_pyproject_toml: poetry_toml::PoetryPyprojectToml =
        toml::from_str(&fs::read_to_string(pyproject_toml)?).with_context(|| {
            format!(
                "Invalid poetry pyproject.toml: {}",
                pyproject_toml.display()
            )
        })?;
    let poetry_section = poetry_pyproject_toml.tool.poetry;

    let root_deps = if no_dev {
        poetry_section.dependencies.clone()
    } else {
        poetry_section
            .dependencies
            .into_iter()
            .chain(poetry_section.dev_dependencies)
            .collect()
    };

    let mut root_extra_deps: HashSet<_> = HashSet::new();
    for extra_name in extras {
        let packages = poetry_section
            .extras
            .get(extra_name)
            .with_context(|| format!("No such extra {}", extra_name))?;
        root_extra_deps.extend(packages);
    }

    let root_deps = root_deps
        .into_iter()
        .filter(|(dep_name, dep_spec)| {
            // We do not need to install python (oh if we only could, relocatable python a dream)
            if dep_name == "python" {
                return false;
            }
            // Use only those optional deps which are activated by a selected extra
            if dep_spec.is_optional() && !root_extra_deps.contains(&dep_name) {
                return false;
            }
            true
        })
        .collect();

    Ok(root_deps)
}

fn get_packages_from_lockfile(pyproject_toml: &Path) -> anyhow::Result<HashMap<String, Package>> {
    // read lockfile
    let lockfile = pyproject_toml
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("poetry.lock");
    let lockfile: poetry_lock::PoetryLock = toml::from_str(&fs::read_to_string(&lockfile)?)
        .with_context(|| format!("Invalid lockfile: {}", lockfile.display()))?;

    // keys are normalized names since `[package.dependencies]` also uses normalized names
    let packages: HashMap<String, poetry_lock::Package> = lockfile
        .package
        .into_iter()
        .map(|package| (package.name.replace('-', "_"), package))
        .collect();
    Ok(packages)
}

/// Parses pyproject.toml and poetry.lock and returns a list of packages to install
pub fn poetry_lockfile_to_specs(
    pyproject_toml: &Path,
    no_dev: bool,
    extras: &[String],
    pep508_env: Option<Pep508Environment>,
) -> anyhow::Result<Vec<Spec>> {
    // TODO: don't parse this from subprocess but do it like maturin
    let environment = pep508_env.unwrap_or_else(Pep508Environment::from_python);

    let root_deps = get_root_info(&pyproject_toml, no_dev, extras)?;

    let packages = get_packages_from_lockfile(pyproject_toml)?;

    // This is the thing we want to build: a list with all transitive dependencies and
    // all their (transitively activated) features
    let mut deps_with_extras: HashMap<String, HashSet<String>> = HashMap::new();
    // (dep, dep->extra) combinations we still need to process
    let mut queue: VecDeque<(String, HashSet<String>)> = VecDeque::new();
    // Since we have no explicit root package, prime manually
    for (dep_name, dep_spec) in root_deps {
        let dep_name_norm = dep_name.to_lowercase().replace("-", "_");

        queue.push_back((
            dep_name_norm.clone(),
            dep_spec.get_extras().iter().cloned().collect(),
        ));
        deps_with_extras.insert(
            dep_name_norm.clone(),
            dep_spec.get_extras().iter().cloned().collect(),
        );
    }

    // resolve the dependencies-extras tree
    // (dep, dep->extra)
    while let Some((dep_name, self_extras)) = queue.pop_front() {
        let package = packages
            .get(&dep_name.to_lowercase().replace('-', "_"))
            .with_context(|| {
                format!(
                    "Lockfile outdated (run `poetry update`): {} is missing",
                    dep_name
                )
            })?;
        // descend one level into the dep tree
        for (new_dep_name, new_dep) in package.dependencies.clone().unwrap_or_default() {
            let new_dep_name_norm = new_dep_name.to_lowercase().replace("-", "_");
            // Check the extras selected on the current dep activate the transitive dependency
            let (_new_dep_version, new_dep_extras) = match new_dep
                .get_version_and_extras(&environment, &self_extras)
                .map_err(WheelInstallerError::InvalidPoetry)?
            {
                None => continue,
                Some((version, new_dep_extras)) => (version, new_dep_extras),
            };

            let new_dep_extras: HashSet<String> = new_dep_extras.into_iter().collect();

            let new_extras = if let Some(known_extras) = deps_with_extras.get(&new_dep_name) {
                if new_dep_extras.is_subset(known_extras) {
                    // nothing to do here, the dep and all those extras are already known
                    continue;
                } else {
                    new_dep_extras.difference(known_extras).cloned().collect()
                }
            } else {
                new_dep_extras
            };

            deps_with_extras
                .entry(new_dep_name_norm.clone())
                .or_default()
                .extend(new_extras.clone());
            queue.push_back((new_dep_name_norm.clone(), new_extras));
        }
    }

    let specs = resolution_to_specs(packages, deps_with_extras)?;
    Ok(specs)
}

#[cfg(test)]
mod test {
    use crate::poetry::parse_dep_extra;
    use std::collections::HashSet;

    #[test]
    fn test_parse_extra_deps() {
        let examples = [
            ("pympler", ("pympler".to_string(), HashSet::new(), None)),
            (
                "pytest (>=4.3.0)",
                (
                    "pytest".to_string(),
                    HashSet::new(),
                    Some(">=4.3.0".to_string()),
                ),
            ),
            (
                "coverage[toml] (>=5.0.2)",
                (
                    "coverage".to_string(),
                    ["toml".to_string()].into_iter().collect(),
                    Some(">=5.0.2".to_string()),
                ),
            ),
        ];

        for (input, expected) in examples {
            assert_eq!(parse_dep_extra(input).unwrap(), expected);
        }
    }
}
