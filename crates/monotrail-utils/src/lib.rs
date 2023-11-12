//! Implements stand-alone utilities used by `monotrail`

pub use requirements_txt::RequirementsTxt;

mod requirements_txt;
pub mod parse_cpython_args;
pub mod standalone_python;
