use crate::WheelInstallerError;
use fs_err as fs;
use std::collections::HashMap;
use std::path::Path;

/// Parse pyvenv.cfg from the root of the virtualenv and returns the python major and minor version
pub fn get_venv_python_version(venv: &Path) -> Result<(u8, u8), WheelInstallerError> {
    let pyvenv_cfg = venv.join("pyvenv.cfg");
    if !pyvenv_cfg.is_file() {
        return Err(WheelInstallerError::BrokenVenv(format!(
            "The virtual environment needs to have a pyvenv.cfg, but {} doesn't exist",
            pyvenv_cfg.display(),
        )));
    }
    get_pyvenv_cfg_python_version(&fs::read_to_string(pyvenv_cfg)?)
}

/// Parse pyvenv.cfg from the root of the virtualenv and returns the python major and minor version
pub fn get_pyvenv_cfg_python_version(pyvenv_cfg: &str) -> Result<(u8, u8), WheelInstallerError> {
    let pyvenv_cfg: HashMap<String, String> = pyvenv_cfg
        .lines()
        // Actual pyvenv.cfg doesn't have trailing newlines, but some program might insert some
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split_once(" = ")
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .ok_or_else(|| WheelInstallerError::BrokenVenv("Invalid pyvenv.cfg".to_string()))
        })
        .collect::<Result<HashMap<String, String>, WheelInstallerError>>()?;

    let version_info = pyvenv_cfg.get("version_info").ok_or_else(|| {
        WheelInstallerError::BrokenVenv("Missing version_info in pyvenv.cfg".to_string())
    })?;
    let python_version: (u8, u8) = match &version_info.split('.').collect::<Vec<_>>()[..] {
        [major, minor, ..] => (
            major.parse().map_err(|err| {
                WheelInstallerError::BrokenVenv(format!(
                    "Invalid major version_info in pyvenv.cfg: {}",
                    err
                ))
            })?,
            minor.parse().map_err(|err| {
                WheelInstallerError::BrokenVenv(format!(
                    "Invalid minor version_info in pyvenv.cfg: {}",
                    err
                ))
            })?,
        ),
        _ => {
            return Err(WheelInstallerError::BrokenVenv(
                "Invalid version_info in pyvenv.cfg".to_string(),
            ))
        }
    };
    Ok(python_version)
}
