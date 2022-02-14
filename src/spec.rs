use crate::wheel_tags::WheelFilename;
use crate::WheelInstallerError;
use regex::Regex;
use std::path::PathBuf;
use std::str::FromStr;

/// We have four sources of package install requests:
///  * User gives a package name (no version), needs json api and download
///  * User gives a package name and version, needs json api and download
///  * User gives a file, which has name and version, doesn't need download
///  * Lockfile fives name, version and filename, needs download
pub struct Spec {
    pub requested: String,
    pub name: String,
    pub version: Option<String>,
    pub file_path: Option<(PathBuf, WheelFilename)>,
    pub url: Option<String>,
}

impl Spec {
    /// Parses "package_name", "package_name==version" and "some/path/tqdm-4.62.3-py2.py3-none-any.whl"
    fn from_requested(requested: impl AsRef<str>) -> Result<Self, WheelInstallerError> {
        if requested.as_ref().ends_with(".whl") {
            let file_path = PathBuf::from(requested.as_ref());
            let filename = file_path
                .file_name()
                .ok_or_else(|| WheelInstallerError::InvalidWheel("Expected a file".to_string()))?
                .to_string_lossy();
            let metadata = WheelFilename::from_str(&filename)?;
            Ok(Spec {
                requested: requested.as_ref().to_string(),
                name: metadata.distribution.clone(),
                version: Some(metadata.version.clone()),
                file_path: Some((file_path, metadata)),
                url: None,
            })
        } else {
            // TODO: check actual naming rules
            let valid_name = Regex::new(r"[-_a-zA-Z0-9.]+").unwrap();
            if let Some((name, version)) = requested.as_ref().split_once("==") {
                Ok(Spec {
                    requested: requested.as_ref().to_string(),
                    name: name.to_string(),
                    version: Some(version.to_string()),
                    file_path: None,
                    url: None,
                })
            } else if valid_name.is_match(requested.as_ref()) {
                Ok(Spec {
                    requested: requested.as_ref().to_string(),
                    name: requested.as_ref().to_string(),
                    version: None,
                    file_path: None,
                    url: None,
                })
            } else {
                Err(WheelInstallerError::Pep440)
            }
        }
    }
}
