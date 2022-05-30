use crate::monotrail::install_specs_to_finder;
use crate::standalone_python::provision_python;
use crate::{get_specs, DEFAULT_PYTHON_VERSION};
use anyhow::{bail, format_err, Context};
use fs_err as fs;
use libc::{c_int, c_void, wchar_t};
use std::collections::BTreeMap;
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use tracing::{debug, trace};
use widestring::WideCString;

/// python has idiosyncratic cli options that are hard to replicate with clap, so we roll our own.
/// Takes args without the first-is-current-program (i.e. python) convention.
///
/// `usage: python [option] ... [-c cmd | -m mod | file | -] [arg] ...`
pub fn naive_python_arg_parser<T: AsRef<str>>(args: &[T]) -> Result<Option<String>, String> {
    let bool_opts = [
        "-b", "-B", "-d", "-E", "-h", "-i", "-I", "-O", "-OO", "-q", "-s", "-S", "-u", "-v", "-V",
        "-x",
    ];
    let arg_opts = ["--check-hash-based-pycs", "-W", "-X"];
    let mut arg_iter = args.iter();
    loop {
        if let Some(arg) = arg_iter.next() {
            if bool_opts.contains(&arg.as_ref()) {
                continue;
            } else if arg_opts.contains(&arg.as_ref()) {
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

/// The way we're using to load symbol by symbol with the type generic is really ugly and cumbersome
/// If you know how to do this with `extern` or even pyo3-ffi directly please tell me
///
/// Returns the exit code from python
pub fn inject_and_run_python(
    python_home: &Path,
    python_version: (u8, u8),
    args: &[String],
    finder_data: &str,
) -> anyhow::Result<c_int> {
    trace!("Loading libpython");
    let libpython3_so = if cfg!(target_os = "macos") {
        python_home.join("lib").join(format!(
            "libpython{}.{}.dylib",
            python_version.0, python_version.1
        ))
    } else {
        python_home.join("lib").join("libpython3.so")
    };
    let lib = {
        #[cfg(unix)]
        {
            let flags = libloading::os::unix::RTLD_LAZY | libloading::os::unix::RTLD_GLOBAL;
            let unix_lib =
                unsafe { libloading::os::unix::Library::open(Some(libpython3_so), flags)? };
            libloading::Library::from(unix_lib)
        }
        // Entirely untested, but it should at least compile
        #[cfg(windows)]
        unsafe {
            libloading::os::unix::Windows::Library::new(libpython3_so)?
        }
    };
    trace!("Initializing libpython");
    unsafe {
        // initialize python
        // otherwise we get an error that it can't find encoding that tells us to set PYTHONHOME
        env::set_var("PYTHONHOME", python_home);
        // TODO: Do this via python c api instead
        env::set_var("PYTHONNOUSERSITE", "1");
        env::set_var("PYTHONUTF8", "1");
        // https://docs.python.org/3/c-api/init.html?highlight=py_initialize#c.Py_Initialize
        // void Py_Initialize()
        let initialize: libloading::Symbol<unsafe extern "C" fn() -> c_void> =
            lib.get(b"Py_Initialize")?;
        initialize();

        debug!("Injecting monotrail");
        // Add our finder
        // https://docs.python.org/3/c-api/veryhigh.html#c.PyRun_String
        // int PyRun_SimpleString(const char *command)
        let run_string: libloading::Symbol<unsafe extern "C" fn(*const char) -> c_int> =
            lib.get(b"PyRun_SimpleString")?;

        // This is a really horrible way to inject that information and it should be done with
        // PyRun_StringFlags instead
        let read_json = "finder_data = FinderData.from_json(finder_data_str)";
        let update_and_activate =
            "MonotrailFinder.get_singleton().update_and_activate(finder_data)";
        let command_str = format!(
            "{}\n{}\nfinder_data_str=r'{}'\n{}\n{}\nmaybe_debug()\n",
            include_str!("../python/monotrail/monotrail_finder.py"),
            include_str!("../python/monotrail/convert_finder_data.py"),
            // TODO: actual encoding strings
            // This just hopefully works because json uses double quotes so there shouldn't
            // be any escaped single quotes in there
            finder_data.replace('\'', r"\u0027"),
            read_json,
            update_and_activate
        );

        let command = CString::new(command_str.clone()).unwrap();
        let result = run_string(command.as_ptr() as *const char);
        if result != 0 {
            debug!("Failing inject code:\n---\n{}---", command_str);
            bail!("Injecting monotrail failed. Try RUST_LOG=debug for more info")
        }

        debug!("Running Py_Main: {}", args.join(" "));
        // run python interpreter as from the cli
        // https://docs.python.org/3/c-api/veryhigh.html#c.Py_BytesMain
        let py_main: libloading::Symbol<unsafe extern "C" fn(c_int, *mut *const wchar_t) -> c_int> =
            lib.get(b"Py_Main")?;

        // env::args panics when there is a non utf-8 string, but converting OsString -> *c_char
        // is an even bigger mess
        let args_cstring: Vec<WideCString> = args
            .iter()
            .map(|arg| WideCString::from_str(&arg).unwrap())
            .collect();
        let mut args_c_char: Vec<*const wchar_t> = args_cstring
            .iter()
            .map(|arg| arg.as_ptr() as *const wchar_t)
            .collect();
        let exit_code = py_main(args_cstring.len() as c_int, args_c_char.as_mut_ptr());
        // > The return value will be 0 if the interpreter exits normally (i.e., without an
        // > exception), 1 if the interpreter exits due to an exception, or 2 if the parameter list
        // > does not represent a valid Python command line.
        // >
        // > Note that if an otherwise unhandled SystemExit is raised, this function will not
        // > return 1, but exit the process, as long as Py_InspectFlag is not set.
        // Let the caller exit with that status if python didn't
        Ok(exit_code)
    }
}

/// Allows doing `monotrail_python +3.10 -m say.hello`
#[allow(clippy::type_complexity)]
pub fn parse_plus_arg(python_args: &[String]) -> anyhow::Result<(Vec<String>, Option<(u8, u8)>)> {
    if let Some(first_arg) = python_args.get(0) {
        if first_arg.starts_with('+') {
            let python_version = parse_major_minor(first_arg)?;
            return Ok((python_args[1..].to_vec(), Some(python_version)));
        }
    }
    Ok((python_args.to_vec(), None))
}

/// Parses "3.8" to (3, 8)
pub fn parse_major_minor(version: &str) -> anyhow::Result<(u8, u8)> {
    let python_version =
        if let Some((major, minor)) = version.trim_start_matches('+').split_once('.') {
            let major = major
                .parse::<u8>()
                .context("Could not parse value of version_major")?;
            let minor = minor
                .parse::<u8>()
                .context("Could not parse value of version_minor")?;
            (major, minor)
        } else {
            bail!("Expect +x.y as first argument (missing dot)");
        };
    Ok(python_version)
}

pub fn run_python_args(
    args: &[String],
    python_version: Option<&str>,
    root: Option<&Path>,
    extras: &[String],
) -> anyhow::Result<i32> {
    let (args, python_version) = determine_python_version(args, python_version)?;

    let script = if let Some(root) = root {
        Some(root.to_path_buf())
    } else {
        naive_python_arg_parser(&args)
            .map_err(|err| format_err!("Failed to parse python args: {}", err))?
            .map(PathBuf::from)
    };
    debug!("run_python_args: {:?}, `{}`", script, args.join(" "));

    let (python_context, python_home) = provision_python(python_version)?;

    let (specs, scripts, lockfile) = get_specs(script.as_deref(), extras, &python_context)?;
    let finder_data = install_specs_to_finder(&specs, scripts, lockfile, None, &python_context)?;

    let args: Vec<_> = [python_context.sys_executable.to_string_lossy().to_string()]
        .into_iter()
        .chain(args)
        .collect();

    let exit_code = inject_and_run_python(
        &python_home,
        python_context.version,
        &args,
        &serde_json::to_string(&finder_data).unwrap(),
    )
    .context("inject and run failed")?;
    if exit_code != 0 {
        debug!("Python didn't exit with code 0: {}", exit_code);
    }
    Ok(exit_code as i32)
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
) -> anyhow::Result<(Vec<String>, (u8, u8))> {
    let (args, python_version_plus) = parse_plus_arg(&python_args)?;
    let python_version_arg = python_version.map(parse_major_minor).transpose()?;
    let env_var = format!("{}_PYTHON_VERSION", env!("CARGO_PKG_NAME").to_uppercase());
    let python_version_env = env::var_os(&env_var)
        .map(|x| parse_major_minor(x.to_string_lossy().as_ref()))
        .transpose()
        .with_context(|| format!("Couldn't parse {}", env_var))?;
    trace!(
        "python versions: as argument: {:?}, with plus: {:?}, with {}: {:?}",
        python_version_plus,
        python_version_arg,
        env_var,
        python_version_env
    );
    let python_version = match (python_version_plus, python_version_arg, python_version_env) {
        (None, None, None) => DEFAULT_PYTHON_VERSION,
        (Some(python_version_plus), None, None) => python_version_plus,
        (None, Some(python_version_arg), None) => python_version_arg,
        (None, None, Some(python_version_env)) => python_version_env,
        (python_version_plus, python_version_arg, python_version_env) => {
            bail!(
                "Conflicting python versions: as argument {:?}, with plus: {:?}, with {}: {:?}",
                python_version_plus,
                python_version_arg,
                env_var,
                python_version_env
            );
        }
    };
    Ok((args, python_version))
}

#[cfg(test)]
mod tests {
    use crate::inject_and_run::naive_python_arg_parser;

    #[test]
    fn test_naive_python_arg_parser() {
        let cases: &[(&[&str], _)] = &[
            (
                &["-v", "-m", "mymod", "--first_arg", "second_arg"],
                Ok(None),
            ),
            (
                &["-v", "my_script.py", "--first_arg", "second_arg"],
                Ok(Some("my_script.py".to_string())),
            ),
            (&["-v"], Ok(None)),
            (&[], Ok(None)),
            (&["-m"], Err("Missing argument for -m".to_string())),
        ];
        for (args, parsing) in cases {
            assert_eq!(&naive_python_arg_parser(args), parsing);
        }
    }
}

/// Extends PATH with a directory containing all the scripts we found. This is because many tools
/// such as jupyter depend on scripts being in path instead of using the python api. It also adds
/// monotrail as python so execve with other scripts works (required e.g. by jupyter)
///
/// You have to pass a tempdir to control its lifetime
pub fn prepare_execve_environment(
    scripts: &BTreeMap<String, PathBuf>,
    root: &Path,
    tempdir: &Path,
    python_version: (u8, u8),
) -> anyhow::Result<()> {
    let path_dir = tempdir.join(format!("{}-scripts-links", env!("CARGO_PKG_NAME")));
    fs::create_dir_all(&path_dir).context("Failed to create scripts PATH dir")?;
    for (script_name, script_path) in scripts {
        #[cfg(unix)]
        {
            fs_err::os::unix::fs::symlink(&script_path, path_dir.join(script_name))
                .context("Failed to create symlink for scripts PATH")?;
        }
        #[cfg(windows)]
        {
            os::windows::fs::symlink_file(&script_path, path_dir.join(script_name))
                .context("Failed to create symlink for scripts PATH")?;
        }
    }

    #[cfg(unix)]
    {
        // We need to allow execve & friends with python scripts, because that's how e.g. jupyter
        // launches the server and kernels. We can't inject monotrail as python through a wrapper
        // script due to https://www.in-ulm.de/~mascheck/various/shebang/#interpreter-script .
        // I also couldn't get env as intermediary (https://unix.stackexchange.com/a/477651/77322)
        // to work, so instead we make the monotrail executable moonlight as python. We detect
        // where we're python before even running clap
        let pythons = [
            "python".to_string(),
            format!("python{}", python_version.0),
            format!("python{}.{}", python_version.0, python_version.1),
        ];
        for python in pythons {
            fs_err::os::unix::fs::symlink(&env::current_exe()?, path_dir.join(python))
                .context("Failed to create symlink for current exe")?;
        }
    }

    // venv/bin/activate also puts venv scripts first. Our python launcher we have to put first
    // anyway to overwrite system python
    let mut path = path_dir.into_os_string();
    path.push(":");
    path.push(env::var_os("PATH").unwrap_or_default());
    env::set_var("PATH", path);

    // Make a execve-spawned monotrail find the configuration we originally read again
    // TODO: Does the subprocess know about the fullpath of the link through which it was called
    //       and can we use that to read those from a file instead which would be more stable
    env::set_var(
        format!("{}_EXECVE_ROOT", env!("CARGO_PKG_NAME").to_uppercase()),
        root,
    );
    env::set_var(
        format!("{}_PYTHON_VERSION", env!("CARGO_PKG_NAME").to_uppercase()),
        format!("{}.{}", python_version.0, python_version.1),
    );

    Ok(())
}
