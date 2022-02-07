pub use crate::install::install_wheel;
use crate::wheel_tags::{Arch, Os};
pub use cli::{run, Cli};
use std::io;
use thiserror::Error;
use zip::result::ZipError;

mod cli;
mod install;
#[cfg(feature = "package_index")]
mod package_index;
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
    #[error("Failed to detect the operating system version")]
    OsVersionDetectionError(#[source] anyhow::Error),
}
