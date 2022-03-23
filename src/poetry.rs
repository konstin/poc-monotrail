use crate::spec::Spec;
use crate::wheel_tags::WheelFilename;
use anyhow::Context;
use fs_err as fs;
use serde::Deserialize;
use std::collections::HashMap;

use std::path::Path;
use std::str::FromStr;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct PoetryLock {
    package: Vec<Package>,
    metadata: Metadata,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct Package {
    name: String,
    version: String,
    description: String,
    category: String,
    optional: bool,
    python_versions: String,
    #[serde(default)]
    extras: HashMap<String, Vec<String>>,
    dependencies: Option<HashMap<String, Dependency>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
#[serde(untagged)]
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
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct Metadata {
    lock_version: String,
    python_versions: String,
    content_hash: String,
    files: HashMap<String, Vec<HashedFile>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct HashedFile {
    file: String,
    hash: String,
}

/// Resolves a single package's filename and url inside a poetry lockfile
fn filename_and_url(
    lockfile: &PoetryLock,
    package: &Package,
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

/// ```toml
/// [tool]
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
struct PoetryPyprojectToml {
    tool: ToolSection,
}

/// ```toml
/// [tool.poetry]
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
struct ToolSection {
    poetry: PoetrySection,
}

/// ```toml
/// [tool.poetry.dependencies]
/// dep1 = "1.2.3"
/// dep2 = { version = "4.5.6", optional = true }
/// ```
#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum Dependency2 {
    Compact(String),
    Expanded { version: String, optional: bool },
}

/// ```toml
/// [tool.poetry.dependencies]
/// [tool.poetry.dev-dependencies]
/// [tool.poetry.extras]
/// ``
#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
struct PoetrySection {
    dependencies: HashMap<String, Dependency2>,
    dev_dependencies: HashMap<String, Dependency2>,
    extras: HashMap<String, Vec<String>>,
}

/*
/// Parses a poetry lockfile
pub fn poetry_pyproject_toml_dependencies(pyproject_toml: &Path) -> (Vec<String>, Vec<String>) {
    let poetry_pyproject_toml: PoetryPyprojectToml =
        toml::from_str(&fs::read_to_string(pyproject_toml)?)
            .with_context(|| format!("Invalid poetry pyproject.toml: {}", lockfile.display()))?;

    let poetry_section = poetry_pyproject_toml.tool.poetry;
    (
        poetry_section.dependencies.keys().collect(),
        poetry_section.dev_dependencies.keys().collect(),
    )
}
*/

/// Parses a poetry lockfile
pub fn specs_from_lockfile(
    lockfile: &Path,
    compatible_tags: &[(String, String, String)],
) -> anyhow::Result<Vec<Spec>> {
    let lockfile: PoetryLock = toml::from_str(&fs::read_to_string(lockfile)?)
        .with_context(|| format!("Invalid lockfile: {}", lockfile.display()))?;
    let mut specs = Vec::new();
    for package in &lockfile.package {
        // TODO: extras
        if package.category != "main" || package.optional {
            continue;
        }
        let (_filename, url) = filename_and_url(&lockfile, package, compatible_tags)?;
        let spec = Spec {
            requested: format!("{} {}", package.name, package.version),
            name: package.name.clone(),
            version: Some(package.version.clone()),
            file_path: None,
            url: Some(url.clone()),
        };
        specs.push(spec);
    }
    Ok(specs)
}
