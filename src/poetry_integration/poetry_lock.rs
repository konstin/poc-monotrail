//! Types for poetry.lock

use anyhow::bail;
use pep508_rs::{MarkerEnvironment, MarkerTree};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PoetryLock {
    pub package: Vec<Package>,
    pub metadata: Metadata,
}

impl PoetryLock {
    pub fn from_str(data: &str) -> anyhow::Result<Self> {
        let lockfile: Self = toml::from_str(data)?;
        if lockfile.metadata.lock_version != "1.1" && lockfile.metadata.lock_version != "2.0" {
            bail!(
                "Unsupported poetry.lock version {}",
                lockfile.metadata.lock_version
            )
        }
        Ok(lockfile)
    }

    /// Abstract over lock_version 1.1 and 2.0, which change in poetry 1.3
    ///
    /// In 1.1 the filenames and the hashes were separately in the metadata table, while in 2.0
    /// its on each package.
    ///
    /// Pass the package name already normalized
    pub fn get_filenames(&self, package_name: &str) -> Option<&Vec<HashedFile>> {
        if let Some(v1_1) = &self.metadata.files {
            return v1_1.get(package_name);
        }
        if let Some(v2_0) = self.package.iter().find(|p| p.name == package_name) {
            return v2_0.files.as_ref();
        }
        // outdated lockfile, to be handled downstream
        None
    }
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
    // https://github.com/alexcrichton/toml-rs/issues/142#issuecomment-279009115
    #[serde(serialize_with = "toml::ser::tables_last")]
    pub dependencies: Option<HashMap<String, Dependency>>,
    pub source: Option<Source>,
    // Only in lock file format 2.0/poetry 1.3 or newer
    pub files: Option<Vec<HashedFile>>,
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
        environment: &MarkerEnvironment,
        self_extras: &HashSet<String>,
    ) -> Result<Option<(String, Vec<String>)>, String> {
        let extra_re = Regex::new(r#"^extra == "([\w\d_-]+)"$"#).unwrap();
        let self_extras_vec: Vec<&str> = self_extras.iter().map(|str| str.as_str()).collect();

        Ok(match self {
            Dependency::Compact(version) => Some((version.to_string(), Vec::new())),
            Dependency::Expanded(DependencyExpanded {
                version,
                markers,
                extras,
            }) => {
                if let Some(markers) = markers {
                    if let Some(captures) = extra_re.captures(markers) {
                        if self_extras.contains(&captures[1].to_string()) {
                            Some((version.to_string(), extras.clone().unwrap_or_default()))
                        } else {
                            None
                        }
                    } else if MarkerTree::from_str(markers)
                        .unwrap()
                        .evaluate(environment, &self_extras_vec)
                    {
                        Some((version.to_string(), extras.clone().unwrap_or_default()))
                    } else {
                        None
                    }
                } else {
                    Some((version.to_string(), extras.clone().unwrap_or_default()))
                }
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
                        if MarkerTree::from_str(markers)
                            .unwrap()
                            .evaluate(environment, &self_extras_vec)
                        {
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

/// `[[package]] [package.source]`
#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct Source {
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
    pub reference: String,
    pub resolved_reference: String,
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
    /// Only in lock_version 1.1, in version 2.0/poetry 1.3 it's in each package
    pub files: Option<HashMap<String, Vec<HashedFile>>>,
}

/// e.g. `{file = "attrs-21.4.0-py2.py3-none-any.whl", hash = "sha256:2d27e3784d7a565d36ab851fe94887c5eccd6a463168875832a1be79c82828b4"}`
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct HashedFile {
    pub file: String,
    pub hash: String,
}

#[cfg(test)]
mod test {
    use crate::poetry_integration::poetry_lock::PoetryLock;
    use std::fs;

    fn get_filenames(filename: &str, package: &str) -> usize {
        PoetryLock::from_str(&fs::read_to_string(filename).unwrap())
            .unwrap()
            .get_filenames(package)
            .unwrap()
            .len()
    }

    #[test]
    fn filenames_1_1_13() {
        assert_eq!(
            get_filenames("test-data/poetry-1.1.13/poetry.lock", "certifi"),
            2
        );
    }

    #[test]
    fn filenames_1_2_0b1() {
        assert_eq!(
            get_filenames("test-data/poetry-1.2.0b1/poetry.lock", "certifi"),
            2
        );
    }

    #[test]
    fn poetry_1_3() {
        assert_eq!(
            get_filenames("test-data/poetry-1.3-django/poetry.lock", "django"),
            2
        );
    }
}
