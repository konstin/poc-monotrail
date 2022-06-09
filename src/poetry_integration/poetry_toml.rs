//! Types for poetry.toml

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// ```toml
/// [build-system]
/// requires = ["poetry-core>=1.0.0"]
/// build-backend = "poetry.core.masonry.api"
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct BuildSystem {
    pub requires: Vec<String>,
    pub build_backend: String,
}

impl Default for BuildSystem {
    fn default() -> Self {
        Self {
            requires: vec!["poetry-core>=1.0.0".to_string()],
            build_backend: "poetry.core.masonry.api".to_string(),
        }
    }
}

/// ```toml
/// [tool]
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct PoetryPyprojectToml {
    pub tool: Option<ToolSection>,
    pub build_system: Option<BuildSystem>,
}

/// ```toml
/// [tool.poetry]
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct ToolSection {
    pub poetry: Option<PoetrySection>,
}

/// ```toml
/// [tool.poetry.dependencies]
/// dep1 = "1.2.3"
/// dep2 = { version = "4.5.6", optional = true }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(untagged, rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum Dependency {
    Compact(String),
    Expanded {
        version: Option<String>,
        optional: Option<bool>,
        extras: Option<Vec<String>>,
        git: Option<String>,
        branch: Option<String>,
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
///
/// Uses `BTreeMap` instead of `HashMap` to ensure we keep the sorting
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct PoetrySection {
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    // https://github.com/alexcrichton/toml-rs/issues/142#issuecomment-279009115
    #[serde(serialize_with = "toml::ser::tables_last")]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(serialize_with = "toml::ser::tables_last")]
    pub dev_dependencies: BTreeMap<String, Dependency>,
    pub extras: Option<BTreeMap<String, Vec<String>>>,
    pub scripts: Option<BTreeMap<String, String>>,
}
