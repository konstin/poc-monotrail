use std::env;
use std::num::ParseIntError;
use thiserror::Error;
use tracing::trace;

#[derive(Debug, Error)]
pub enum ParsePythonVersionError {
    #[error("Could not parse number in version '{input}'")]
    ParseInt {
        input: String,
        source: ParseIntError,
    },
    #[error("Missing dot in python version argument '{input}'")]
    MissingDot { input: String },
    #[error(
        "Conflicting python versions: as argument {python_version_arg:?}, with plus: \
        {python_version_plus:?}, with {env_var}: {python_version_env:?}"
    )]
    ConflictingPythonVersion {
        python_version_arg: Option<(u8, u8)>,
        python_version_plus: Option<(u8, u8)>,
        env_var: String,
        python_version_env: Option<(u8, u8)>,
    },
}

/// python has idiosyncratic cli options that are hard to replicate with clap, so we roll our own.
/// Takes args without the first-is-current-program (i.e. python) convention.
///
/// `usage: python [option] ... [-c cmd | -m mod | file | -] [arg] ...`
///
/// Returns the script, if any
pub fn naive_python_arg_parser<T: AsRef<str>>(args: &[T]) -> Result<Option<String>, String> {
    // These are hand collected from `python --help`
    // See also https://docs.python.org/3/using/cmdline.html#command-line
    let no_value_opts = [
        "-b", "-B", "-d", "-E", "-h", "-i", "-I", "-O", "-OO", "-q", "-s", "-S", "-u", "-v", "-V",
        "-x", "-?",
    ];
    let value_opts = ["--check-hash-based-pycs", "-W", "-X"];
    let mut arg_iter = args.iter();
    loop {
        if let Some(arg) = arg_iter.next() {
            if no_value_opts.contains(&arg.as_ref()) {
                continue;
            } else if value_opts.contains(&arg.as_ref()) {
                // consume the value belonging to the options
                let value = arg_iter.next();
                if value.is_none() {
                    return Err(format!("Missing argument for {}", arg.as_ref()));
                }
                continue;
            } else if arg.as_ref() == "-c" || arg.as_ref() == "-m" {
                let value = arg_iter.next();
                if value.is_none() {
                    return Err(format!("Missing argument for {}", arg.as_ref()));
                }
                return Ok(None);
            } else {
                return Ok(Some(arg.as_ref().to_string()));
            }
        } else {
            // interactive python shell
            return Ok(None);
        }
    }
}

/// Allows linking monotrail as python and then doing `python +3.10 -m say.hello`
#[allow(clippy::type_complexity)]
pub fn parse_plus_arg(
    python_args: &[String],
) -> Result<(Vec<String>, Option<(u8, u8)>), ParsePythonVersionError> {
    if let Some(first_arg) = python_args.first() {
        if first_arg.starts_with('+') {
            let python_version = parse_major_minor(first_arg)?;
            return Ok((python_args[1..].to_vec(), Some(python_version)));
        }
    }
    Ok((python_args.to_vec(), None))
}

/// Parses "3.8" to (3, 8)
pub fn parse_major_minor(version: &str) -> Result<(u8, u8), ParsePythonVersionError> {
    let Some((major, minor)) = version.trim_start_matches('+').split_once('.') else {
        return Err(ParsePythonVersionError::MissingDot {
            input: version.to_string(),
        });
    };
    let major = major
        .parse::<u8>()
        .map_err(|err| ParsePythonVersionError::ParseInt {
            input: version.to_string(),
            source: err,
        })?;
    let minor = minor
        .parse::<u8>()
        .map_err(|err| ParsePythonVersionError::ParseInt {
            input: version.to_string(),
            source: err,
        })?;

    Ok((major, minor))
}

/// There are three possible sources of a python version:
///  - explicitly as cli argument
///  - as +x.y in the python args
///  - through MONOTRAIL_PYTHON_VERSION, as forwarding through calling our python hook (TODO: give
///    version info to the python hook, maybe with /usr/bin/env, but i don't know how)
/// We ensure that only one is set a time
pub fn determine_python_version(
    python_args: &[String],
    python_version: Option<&str>,
    default_python_version: (u8, u8),
) -> Result<(Vec<String>, (u8, u8)), ParsePythonVersionError> {
    let (args, python_version_plus) = parse_plus_arg(python_args)?;
    let python_version_arg = python_version.map(parse_major_minor).transpose()?;
    let env_var = format!("{}_PYTHON_VERSION", env!("CARGO_PKG_NAME").to_uppercase());
    let python_version_env = env::var_os(&env_var)
        .map(|x| parse_major_minor(x.to_string_lossy().as_ref()))
        .transpose()?;
    trace!(
        "python versions: as argument: {:?}, with plus: {:?}, with {}: {:?}",
        python_version_plus,
        python_version_arg,
        env_var,
        python_version_env
    );
    let python_version = match (python_version_plus, python_version_arg, python_version_env) {
        (None, None, None) => default_python_version,
        (Some(python_version_plus), None, None) => python_version_plus,
        (None, Some(python_version_arg), None) => python_version_arg,
        (None, None, Some(python_version_env)) => python_version_env,
        (python_version_plus, python_version_arg, python_version_env) => {
            return Err(ParsePythonVersionError::ConflictingPythonVersion {
                python_version_plus,
                python_version_arg,
                env_var,
                python_version_env,
            });
        }
    };
    Ok((args, python_version))
}
