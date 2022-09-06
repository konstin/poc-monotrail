//! Types for poetry.lock

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
    // https://github.com/alexcrichton/toml-rs/issues/142#issuecomment-279009115
    #[serde(serialize_with = "toml::ser::tables_last")]
    pub dependencies: Option<HashMap<String, Dependency>>,
    pub source: Option<Source>,
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
                        if self_extras.contains(&captures[1].to_string()) {
                            Some((version.to_string(), extras.clone().unwrap_or_default()))
                        } else {
                            None
                        }
                    } else if parse_markers(markers).unwrap().evaluate(environment)? {
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
