//! Takes a wheel and installs it, either in a venv or for monotrail

// The pub ones are reused by monotrail
pub use install_location::{normalize_name, InstallLocation, LockedDir};
use std::io;
use thiserror::Error;
pub use wheel::{
    get_script_launcher, install_wheel, parse_key_value_file, read_record_file, relative_to,
    Script, MONOTRAIL_SCRIPT_SHEBANG,
};
pub use wheel_tags::{compatible_tags, Arch, Os, WheelFilename};
use zip::result::ZipError;

mod install_location;
#[cfg(feature = "python_bindings")]
mod python_bindings;
mod wheel;
mod wheel_tags;

#[derive(Error, Debug)]
pub enum WheelInstallerError {
    #[error(transparent)]
    IOError(#[from] io::Error),
    /// This shouldn't actually be possible to occur
    #[error("Failed to serialize direct_url.json ಠ_ಠ")]
    DirectUrlSerdeJsonError(#[source] serde_json::Error),
    /// Tags/metadata didn't match platform
    #[error("The wheel is incompatible with the current platform {os} {arch}")]
    IncompatibleWheel { os: Os, arch: Arch },
    /// The wheel is broken
    #[error("The wheel is invalid: {0}")]
    InvalidWheel(String),
    /// pyproject.toml or poetry.lock are broken
    #[error("The poetry dependency specification (pyproject.toml or poetry.lock) is broken (try `poetry update`?): {0}")]
    InvalidPoetry(String),
    /// Doesn't follow file name schema
    #[error("The wheel filename \"{0}\" is invalid: {1}")]
    InvalidWheelFileName(String, String),
    /// The wheel is broken, but in python pkginfo
    #[error("The wheel is broken")]
    PkgInfoError(#[from] python_pkginfo::Error),
    #[error("Failed to read the wheel file")]
    ZipError(#[from] ZipError),
    #[error("Failed to run python subcommand")]
    PythonSubcommandError(#[source] io::Error),
    #[error("Failed to move data files")]
    WalkDirError(#[source] walkdir::Error),
    #[error("RECORD file doesn't match wheel contents: {0}")]
    RecordFileError(String),
    #[error("RECORD file is invalid")]
    RecordCsvError(#[from] csv::Error),
    #[error("Broken virtualenv: {0}")]
    BrokenVenv(String),
    #[error("Failed to detect the operating system version: {0}")]
    OsVersionDetectionError(String),
    #[error("Invalid version specification, only none or == is supported")]
    Pep440,
}
