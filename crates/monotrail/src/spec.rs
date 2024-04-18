//! Descriptions of user requests ([RequestedSpec]) and a fully resolved installable
//! ([ResolvedSpec]).

use crate::package_index::search_release;
use install_wheel_rs::{CompatibleTags, Error, WheelFilename};
use pep508_rs::{ExtraName, PackageName};
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
    /// Will be printed with the error message to indicate what was tried to install
    pub requested: String,
    /// The name of the package
    pub name: PackageName,
    /// The version of the package
    pub python_version: Option<String>,
    pub source: Option<SpecSource>,
    /// The extras of the package to also be installed
    pub extras: Vec<ExtraName>,
    /// TODO: allow sdist filepath
    pub file_path: Option<(PathBuf, WheelFilename)>,
    /// Url, filename, distribution type
    pub url: Option<(String, String, DistributionType)>,
}

impl RequestedSpec {
    pub fn get_unique_version(&self) -> Option<String> {
        if let Some(source) = &self.source {
            Some(source.resolved_reference.clone())
        } else {
            self.python_version.clone()
        }
    }

    /// Parses "package_name", "package_name==version" and "some/path/tqdm-4.62.3-py2.py3-none-any.whl"
    pub fn from_requested(requested: impl AsRef<str>, extras: &[ExtraName]) -> Result<Self, Error> {
        if requested.as_ref().ends_with(".whl") {
            let file_path = PathBuf::from(requested.as_ref());
            let filename = file_path
                .file_name()
                .ok_or_else(|| Error::InvalidWheel("Expected a file".to_string()))?
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
            if let Some((name, version)) = requested.as_ref().split_once("==") {
                Ok(Self {
                    requested: requested.as_ref().to_string(),
                    name: PackageName::from_str(name)?,
                    python_version: Some(version.to_string()),
                    source: None,
                    extras: extras.to_vec(),
                    file_path: None,
                    url: None,
                })
            } else if let Ok(name) = PackageName::from_str(requested.as_ref()) {
                Ok(Self {
                    requested: requested.as_ref().to_string(),
                    name,
                    python_version: None,
                    source: None,
                    extras: extras.to_vec(),
                    file_path: None,
                    url: None,
                })
            } else {
                Err(Error::Pep440)
            }
        }
    }

    /// if required (most cases) it queries the pypi index for the actual url
    /// (the pypi url shortcut doesn't work)
    pub fn resolve(
        &self,
        host: &str,
        compatible_tags: &CompatibleTags,
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

        let (picked_release, distribution_type, version) = search_release(
            host,
            &self.name,
            self.python_version.clone(),
            compatible_tags,
        )?;
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

/// The three places we can install something from: A file (wheel or sdist), a url which offers a
/// file (i.e. the pypi servers) or a git repository.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FileOrUrl {
    File(PathBuf),
    Url { url: String, filename: String },
    Git { url: String, revision: String },
}

/// An installation request for a specific source, that unlike [RequestedSpec] definitely
/// has a version and a location
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedSpec {
    pub requested: String,
    pub name: PackageName,
    /// The pep440 version as importlib.metadata sees it
    pub python_version: String,
    /// A (hopefully) unique identifier for that package. This is the same as python_version
    /// for pypi downloaded wheel, or a git hash for git installs
    ///
    /// We serialize the version to a (hopefully) unique string
    /// TODO: Make sure it's actually unique and document how we do that  
    pub unique_version: String,
    pub extras: Vec<ExtraName>,
    pub location: FileOrUrl,
    pub distribution_type: DistributionType,
}

#[cfg(test)]
mod test {
    use crate::markers::marker_environment_from_json_str;
    use crate::poetry_integration::read_dependencies::poetry_spec_from_dir;
    use crate::spec::{FileOrUrl, ResolvedSpec};
    use crate::utils::zstd_json_mock;
    use install_wheel_rs::{Arch, CompatibleTags, Os};
    use mockito::Server;
    use pep508_rs::PackageName;
    use std::path::Path;
    use std::str::FromStr;

    fn manylinux_url(host: &str, package: &str) -> anyhow::Result<ResolvedSpec> {
        let package = PackageName::from_str(package).unwrap();
        let os = Os::Manylinux {
            major: 2,
            minor: 27,
        };
        let arch = Arch::X86_64;
        let python_version = (3, 7);
        let compatible_tags = CompatibleTags::new(python_version, os, arch).unwrap();
        let pep508_env = marker_environment_from_json_str(
            r##"{
                "implementation_name": "cpython", 
                "implementation_version": "3.7.13",
                "os_name": "posix",
                "platform_machine": "x86_64",
                "platform_python_implementation": "CPython",
                "platform_release": "5.4.188+",
                "platform_system": "Linux",
                "platform_version": "#1 SMP Sun Apr 24 10:03:06 PDT 2022",
                "python_full_version": "3.7.13",
                "python_version": "3.7",
                "sys_platform": "linux"
            }"##,
        );

        let (specs, _, _) = poetry_spec_from_dir(
            Path::new("../../resources/poetry_boostrap_lock"),
            &[],
            &pep508_env,
        )
        .unwrap();
        specs
            .iter()
            .find(|spec| spec.name == package)
            .unwrap()
            .resolve(host, &compatible_tags)
    }

    #[test]
    fn test_manylinux_url() {
        let (server, _mock) =
            zstd_json_mock("/pypi/cffi/json", "../../test-data/pypi/cffi.json.zstd");
        assert_eq!(
            manylinux_url(&server.url(), "cffi").unwrap().location,
            FileOrUrl::Url {
                url: "https://files.pythonhosted.org/packages/93/d0/2e2b27ea2f69b0ec9e481647822f8f77f5fc23faca2dd00d1ff009940eb7/cffi-1.15.1-cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64.whl".to_string(),
                filename: "cffi-1.15.1-cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64.whl".to_string()
            }
        )
    }

    #[test]
    fn test_pypi_no_internet() {
        let server = Server::new();
        // We must use a different package here or we race with the other mock
        let err = manylinux_url(&server.url(), "certifi").unwrap_err();
        let errors = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        // the second message has the mockito url in it
        assert_eq!(
            errors[0],
            "Failed to contact pypi. Is your internet connection working?"
        );
        assert_eq!(errors.len(), 2);
    }
}
