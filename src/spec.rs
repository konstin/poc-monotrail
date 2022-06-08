use crate::package_index::search_release;
use install_wheel_rs::{WheelFilename, WheelInstallerError};
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
    pub python_version: Option<String>,
    pub source: Option<SpecSource>,
    pub extras: Vec<String>,
    /// TODO: allow sdist filepath
    pub file_path: Option<(PathBuf, WheelFilename)>,
    /// Url, filename, distribution type
    pub url: Option<(String, String, DistributionType)>,
}

impl RequestedSpec {
    pub fn normalized_name(&self) -> String {
        self.name.to_lowercase().replace('-', "_")
    }

    pub fn get_unique_version(&self) -> Option<String> {
        if let Some(source) = &self.source {
            Some(source.resolved_reference.clone())
        } else {
            self.python_version.clone()
        }
    }

    /// Parses "package_name", "package_name==version" and "some/path/tqdm-4.62.3-py2.py3-none-any.whl"
    pub fn from_requested(
        requested: impl AsRef<str>,
        extras: &[String],
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
                python_version: Some(metadata.version.clone()),
                source: None,
                extras: extras.to_vec(),
                file_path: Some((file_path, metadata)),
                url: None,
            })
        } else {
            // TODO: check actual naming rules
            let valid_name = Regex::new(r"[-_a-zA-Z\d.]+").unwrap();
            if let Some((name, version)) = requested.as_ref().split_once("==") {
                Ok(Self {
                    requested: requested.as_ref().to_string(),
                    name: name.to_string(),
                    python_version: Some(version.to_string()),
                    source: None,
                    extras: extras.to_vec(),
                    file_path: None,
                    url: None,
                })
            } else if valid_name.is_match(requested.as_ref()) {
                Ok(Self {
                    requested: requested.as_ref().to_string(),
                    name: requested.as_ref().to_string(),
                    python_version: None,
                    source: None,
                    extras: extras.to_vec(),
                    file_path: None,
                    url: None,
                })
            } else {
                Err(WheelInstallerError::Pep440)
            }
        }
    }

    /// if required (most cases) it queries the pypi index for the actual url
    /// (the pypi url shortcut doesn't work)
    pub fn resolve(
        &self,
        compatible_tags: &[(String, String, String)],
    ) -> anyhow::Result<ResolvedSpec> {
        if let Some(python_version) = self.python_version.clone() {
            if let Some((file_path, _filename)) = self.file_path.clone() {
                return Ok(ResolvedSpec {
                    requested: self.requested.clone(),
                    name: self.name.clone(),
                    python_version: python_version.clone(),
                    // TODO: hash path + last modified into something unique
                    unique_version: python_version,
                    extras: self.extras.clone(),
                    location: FileOrUrl::File(file_path),
                    distribution_type: DistributionType::Wheel,
                });
            } else if let Some((url, filename, distribution_type)) = self.url.clone() {
                return Ok(ResolvedSpec {
                    requested: self.requested.clone(),
                    name: self.name.clone(),
                    python_version: python_version.clone(),
                    unique_version: self.get_unique_version().unwrap_or(python_version),
                    extras: self.extras.clone(),
                    location: FileOrUrl::Url { url, filename },
                    distribution_type,
                });
            } else if let Some(source) = self.source.clone() {
                return Ok(ResolvedSpec {
                    requested: self.requested.clone(),
                    name: self.name.clone(),
                    python_version,
                    unique_version: source.resolved_reference.clone(),
                    extras: self.extras.clone(),
                    location: FileOrUrl::Git {
                        url: source.url,
                        revision: source.resolved_reference,
                    },
                    distribution_type: DistributionType::SourceDistribution,
                });
            }
        }

        let (picked_release, distribution_type, version) =
            search_release(&self.name, self.python_version.clone(), compatible_tags)?;
        Ok(ResolvedSpec {
            requested: self.requested.clone(),
            name: self.name.clone(),
            python_version: version.clone(),
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
    Git { url: String, revision: String },
}

/// An installation request for a specific source, that unlike [RequestedSpec] definitely
/// has a version
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedSpec {
    pub requested: String,
    pub name: String,
    /// The pep440 version as importlib.metadata sees it
    pub python_version: String,
    /// A (hopefully) unique identifier for that package. This is the same as python_version
    /// for pypi downloaded wheel, or a git hash for git installs
    ///
    /// We serialize the version to a (hopefully) unique string
    /// TODO: Make sure it's actually unique and document how we do that  
    pub unique_version: String,
    pub extras: Vec<String>,
    pub location: FileOrUrl,
    pub distribution_type: DistributionType,
}

#[cfg(test)]
mod test {
    use crate::spec::{FileOrUrl, ResolvedSpec};
    use crate::utils::zstd_json_mock;
    use crate::{poetry_spec_from_dir, Pep508Environment};
    use install_wheel_rs::{compatible_tags, Arch, Os};
    use std::path::Path;

    fn manylinux_url(package: &str) -> anyhow::Result<ResolvedSpec> {
        let os = Os::Manylinux {
            major: 2,
            minor: 27,
        };
        let arch = Arch::X86_64;
        let python_version = (3, 7);
        let compatible_tags = compatible_tags(python_version, &os, &arch).unwrap();
        let pep508_env = Pep508Environment::from_json_str(
            r##"{"implementation_name": "cpython", "implementation_version": "3.7.13", "os_name": "posix", "platform_machine": "x86_64", "platform_python_implementation": "CPython", "platform_release": "5.4.188+", "platform_system": "Linux", "platform_version": "#1 SMP Sun Apr 24 10:03:06 PDT 2022", "python_full_version": "3.7.13", "python_version": "3.7", "sys_platform": "linux"}"##,
        );

        let (specs, _, _) = poetry_spec_from_dir(
            Path::new("src/poetry_integration/poetry_boostrap_lock"),
            &[],
            &pep508_env,
        )
        .unwrap();
        specs
            .iter()
            .find(|spec| spec.name == package)
            .unwrap()
            .resolve(&compatible_tags)
    }

    #[test]
    fn test_manylinux_url() {
        let _mock = zstd_json_mock("/pypi/cffi/json", "test-data/pypi/cffi.json.zstd");
        assert_eq!(
            manylinux_url("cffi").unwrap().location,
            FileOrUrl::Url {
                url: "https://files.pythonhosted.org/packages/44/6b/5edf93698ef1dc745774e47e26f5995040dd3604562dd63f5959fcd3a49e/cffi-1.15.0-cp37-cp37m-manylinux_2_12_x86_64.manylinux2010_x86_64.whl".to_string(),
                filename: "cffi-1.15.0-cp37-cp37m-manylinux_2_12_x86_64.manylinux2010_x86_64.whl".to_string()
            },
        )
    }

    #[test]
    fn test_pypi_no_internet() {
        // We must use a different package here or we race with the other mock
        let err = manylinux_url("certifi").unwrap_err();
        let errors = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        // the second message has the mockito url in it
        assert_eq!(
            errors[0],
            "Failed to contact pypi. Is your internet connection working?"
        );
        assert_eq!(errors.len(), 2);
    }
}
