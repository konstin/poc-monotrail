use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

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

pub fn get_install_list(lockfile: &Path) -> anyhow::Result<()> {
    let lockfile: PoetryLock = toml::from_str(&fs_err::read_to_string(lockfile)?)
        .with_context(|| format!("Invalid lockfile: {}", lockfile.display()))?;
    for package in lockfile.package {
        println!("{} {}", package.name, package.version);
    }
    Ok(())
}
