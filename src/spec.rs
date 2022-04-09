use crate::package_index::search_release;
use crate::wheel_tags::WheelFilename;
use crate::WheelInstallerError;
use regex::Regex;
use std::path::PathBuf;
use std::str::FromStr;

/// Additional metadata for the url
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DistributionType {
    Wheel,
    SourceDistribution,
}

/// Same type as from poetry but separate to not bind to strongly to poetry
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SpecSource {
    pub source_type: String,
    pub url: String,
    pub reference: String,
    pub resolved_reference: String,
}

/// We have four sources of package install requests:
///  * User gives a package name (no version), needs json api and download
///  * User gives a package name and version, needs json api and download
///  * User gives a file, which has name and version, doesn't need download
///  * Lockfile fives name, version and filename, needs download
///
/// TODO: carry hashes/locked files
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RequestedSpec {
    pub requested: String,
    pub name: String,
    pub version: Option<String>,
    pub source: Option<SpecSource>,
    pub extras: Vec<String>,
    /// TODO: allow sdist filepath
    pub file_path: Option<(PathBuf, WheelFilename)>,
    /// Url, filename, distribution type
    pub url: Option<(String, String, DistributionType)>,
}

impl RequestedSpec {
    pub fn resolve(
        &self,
        compatible_tags: &[(String, String, String)],
    ) -> anyhow::Result<ResolvedSpec> {
        if let Some(version) = self.version.clone() {
            if let Some((file_path, _filename)) = self.file_path.clone() {
                return Ok(ResolvedSpec {
                    requested: self.requested.clone(),
                    name: self.name.clone(),
                    version: version.clone(),
                    // TODO: hash path + last modified into something unique
                    unique_version: version,
                    extras: self.extras.clone(),
                    location: FileOrUrl::File(file_path),
                    distribution_type: DistributionType::Wheel,
                });
            } else if let Some((url, filename, distribution_type)) = self.url.clone() {
                return Ok(ResolvedSpec {
                    requested: self.requested.clone(),
                    name: self.name.clone(),
                    version: version.clone(),
                    unique_version: if let Some(source) = &self.source {
                        source.resolved_reference.clone()
                    } else {
                        version.clone()
                    },
                    extras: self.extras.clone(),
                    location: FileOrUrl::Url { url, filename },
                    distribution_type,
                });
            }
        }

        let (picked_release, distribution_type, version) =
            search_release(&self.name, self.version.clone(), compatible_tags)?;
        Ok(ResolvedSpec {
            requested: self.requested.clone(),
            name: self.name.clone(),
            version: version.clone(),
            unique_version: version,
            extras: self.extras.clone(),
            location: FileOrUrl::Url {
                url: picked_release.url,
                filename: picked_release.filename,
            },
            distribution_type,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FileOrUrl {
    File(PathBuf),
    Url { url: String, filename: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedSpec {
    pub requested: String,
    pub name: String,
    pub version: String,
    /// We serialize the version to a (hopefully) unique string
    /// TODO: Make sure it's actually unique and document how we do that  
    pub unique_version: String,
    pub extras: Vec<String>,
    pub location: FileOrUrl,
    pub distribution_type: DistributionType,
}

impl RequestedSpec {
    /// Parses "package_name", "package_name==version" and "some/path/tqdm-4.62.3-py2.py3-none-any.whl"
    pub fn from_requested(
        requested: impl AsRef<str>,
        extras: Vec<String>,
    ) -> Result<Self, WheelInstallerError> {
        if requested.as_ref().ends_with(".whl") {
            let file_path = PathBuf::from(requested.as_ref());
            let filename = file_path
                .file_name()
                .ok_or_else(|| WheelInstallerError::InvalidWheel("Expected a file".to_string()))?
                .to_string_lossy();
            let metadata = WheelFilename::from_str(&filename)?;
            Ok(Self {
                requested: requested.as_ref().to_string(),
                name: metadata.distribution.clone(),
                version: Some(metadata.version.clone()),
                source: None,
                extras,
                file_path: Some((file_path, metadata)),
                url: None,
            })
        } else {
            // TODO: check actual naming rules
            let valid_name = Regex::new(r"[-_a-zA-Z0-9.]+").unwrap();
            if let Some((name, version)) = requested.as_ref().split_once("==") {
                Ok(Self {
                    requested: requested.as_ref().to_string(),
                    name: name.to_string(),
                    version: Some(version.to_string()),
                    source: None,
                    extras,
                    file_path: None,
                    url: None,
                })
            } else if valid_name.is_match(requested.as_ref()) {
                Ok(Self {
                    requested: requested.as_ref().to_string(),
                    name: requested.as_ref().to_string(),
                    version: None,
                    source: None,
                    extras,
                    file_path: None,
                    url: None,
                })
            } else {
                Err(WheelInstallerError::Pep440)
            }
        }
    }
}
