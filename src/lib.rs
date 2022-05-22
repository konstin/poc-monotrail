#![allow(clippy::needless_borrow)] // This is really annoying when refactoring

use crate::install::install_specs;
pub use crate::markers::Pep508Environment;
pub use crate::monotrail::get_specs;
pub use cli::{run_cli, Cli};
pub use inject_and_run::run_python_args;
use poetry_integration::read_dependencies::read_poetry_specs;

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
