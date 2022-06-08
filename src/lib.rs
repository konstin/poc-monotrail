#![allow(clippy::needless_borrow)] // This is really annoying when refactoring

use crate::install::install_specs;
pub use crate::markers::Pep508Environment;
pub use crate::monotrail::get_specs;
pub use cli::{run_cli, Cli};
pub use inject_and_run::{parse_major_minor, run_python_args};
pub use poetry_integration::read_dependencies::poetry_spec_from_dir;
use poetry_integration::read_dependencies::read_poetry_specs;
#[doc(hidden)]
pub use utils::assert_cli_error;

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
mod utils;
mod venv_parser;
mod verify_installation;

pub static PEP508_QUERY_ENV: &str = include_str!("get_pep508_env.py");
pub const DEFAULT_PYTHON_VERSION: (u8, u8) = (3, 8);
