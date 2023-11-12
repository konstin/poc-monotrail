//! Implements stand-alone utilities used by `monotrail`

pub use requirements_txt::RequirementsTxt;

pub mod parse_cpython_args;
mod requirements_txt;
pub mod standalone_python;
