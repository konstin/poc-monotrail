use crate::markers::{parse_markers, PythonEnvironment};
use crate::poetry::poetry_lock::PoetryLock;
use crate::spec::Spec;
use crate::wheel_tags::WheelFilename;
use anyhow::Context;
use fs_err as fs;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::str::FromStr;

mod poetry_lock {
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct PoetryLock {
        pub package: Vec<Package>,
        pub metadata: Metadata,
    }

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

    #[derive(Deserialize, Debug, Clone)]
    #[serde(untagged, rename_all = "kebab-case")]
    pub enum Dependency {
        Compact(String),
        Expanded { version: String, markers: String },
    }

    #[allow(dead_code)]
    impl Dependency {
        pub fn get_version(&self) -> &str {
            match self {
                Dependency::Compact(version) => version,
                Dependency::Expanded { version, .. } => version,
            }
        }
        pub fn get_markers(&self) -> Option<&str> {
            match self {
                Dependency::Compact(_) => None,
                Dependency::Expanded { markers, .. } => Some(markers),
            }
        }
    }

    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct Metadata {
        lock_version: String,
        python_versions: String,
        content_hash: String,
        pub(crate) files: HashMap<String, Vec<HashedFile>>,
    }

    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
    pub struct HashedFile {
        pub(crate) file: String,
        hash: String,
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
    #[allow(dead_code)]
    pub struct PoetryPyprojectToml {
        pub tool: ToolSection,
    }

    /// ```toml
    /// [tool.poetry]
    /// ```
    #[derive(Deserialize, Debug, Clone)]
    #[serde(rename_all = "kebab-case")]
    #[allow(dead_code)]
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
        Expanded { version: String, optional: bool },
    }

    impl Dependency {
        pub fn is_optional(&self) -> bool {
            match self {
                Dependency::Compact(_) => false,
                Dependency::Expanded { optional, .. } => *optional,
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
) -> anyhow::Result<(String, String)> {
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
    let (filename, parsed_filename) = filenames
        .into_iter()
        .find(|(_filename, parsed)| parsed.is_compatible(compatible_tags))
        .with_context(|| {
            format!(
                "No compatible compiled file found for {}. \
                    Is it missing a wheel for your operating system/architecture/python version?",
                package.name
            )
        })?;

    // https://warehouse.pypa.io/api-reference/integration-guide.html#if-you-so-choose
    let url = format!(
        "https://files.pythonhosted.org/packages/{}/{}/{}/{}",
        parsed_filename.python_tag.join("."),
        package.name.chars().next().unwrap(),
        package.name,
        filename,
    );
    Ok((filename, url))
}

pub fn find_specs_to_install(
    pyproject_toml: &Path,
    compatible_tags: &[(String, String, String)],
    no_dev: bool,
    extras: &[String],
) -> anyhow::Result<Vec<Spec>> {
    // TODO: don't parse this from env but do it like maturin
    let environment = PythonEnvironment::from_python();

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
    let packages: HashMap<&str, &poetry_lock::Package> = lockfile
        .package
        .iter()
        .map(|package| (package.name.as_str(), package))
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
        let package = packages.get(item.as_str()).with_context(|| {
            format!(
                "Lockfile outdated (run `poetry update`): {} is missing",
                item
            )
        })?;
        let (_filename, url) = filename_and_url(&lockfile, package, compatible_tags)?;
        let spec = Spec {
            requested: format!("{} {}", package.name, package.version),
            name: package.name.clone(),
            version: Some(package.version.clone()),
            file_path: None,
            url: Some(url.clone()),
        };
        specs.push(spec);

        // 2. Add package's deps to queue (basically recursion)
        for (item, dependency) in package.dependencies.as_ref().unwrap_or(&HashMap::new()) {
            if seen.contains(item) {
                continue;
            }
            if let Some(markers) = dependency.get_markers() {
                if markers.starts_with("extra ==") {
                    todo!("markers extra");
                }
                if !parse_markers(markers).unwrap().evaluate(&environment) {
                    continue;
                }
            }

            queue.push_back(item.clone());
            seen.insert(item.clone());
        }
    }

    Ok(specs)
}
