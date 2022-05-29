#![allow(clippy::needless_borrow)] // This is really annoying when refactoring

use crate::install::install_specs;
pub use crate::markers::Pep508Environment;
pub use crate::monotrail::get_specs;
pub use cli::{run_cli, Cli};
pub use inject_and_run::{parse_major_minor, run_python_args};
use install_wheel_rs::WheelInstallerError;
use poetry_integration::read_dependencies::read_poetry_specs;
use std::path::PathBuf;
use std::{io, result};

mod cli;
mod inject_and_run;
mod install;
mod markers;
mod monotrail;
mod package_index;
mod poetry_integration;
#[cfg(feature = "python_bindings")]
mod python_bindings;
mod requirements_txt;
mod source_distribution;
mod spec;
mod standalone_python;
mod venv_parser;

pub static PEP508_QUERY_ENV: &str = include_str!("get_pep508_env.py");
pub const DEFAULT_PYTHON_VERSION: (u8, u8) = (3, 8);

/// `~/.cache/monotrail`
pub(crate) fn cache_dir() -> result::Result<PathBuf, WheelInstallerError> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| {
            WheelInstallerError::IOError(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a cache dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}

/// `~/.local/monotrail`
pub(crate) fn data_local_dir() -> result::Result<PathBuf, WheelInstallerError> {
    Ok(dirs::data_local_dir()
        .ok_or_else(|| {
            WheelInstallerError::IOError(io::Error::new(
                io::ErrorKind::NotFound,
                "System needs to have a data dir",
            ))
        })?
        .join(env!("CARGO_PKG_NAME")))
}
