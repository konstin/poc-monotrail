//! Communication with libpython

use crate::monotrail::{find_scripts, install, load_specs, FinderData, InjectData, PythonContext};
use crate::standalone_python::provision_python;
use crate::DEFAULT_PYTHON_VERSION;
use anyhow::{bail, format_err, Context};
use fs_err as fs;
use install_wheel_rs::{get_script_launcher, Script, MONOTRAIL_SCRIPT_SHEBANG};
use libc::{c_int, c_void, wchar_t};
use libloading::Library;
use std::collections::BTreeMap;
use std::env;
use std::env::current_exe;
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::{debug, trace};
use widestring::WideCString;

/// <https://docs.python.org/3/c-api/init_config.html#preinitialize-python-with-pypreconfig>
///
/// <https://docs.rs/pyo3/0.16.5/pyo3/ffi/struct.PyPreConfig.html>
#[repr(C)]
#[derive(Debug)]
pub struct PyPreConfig {
    pub _config_init: c_int,
    pub parse_argv: c_int,
    pub isolated: c_int,
    pub use_environment: c_int,
    pub configure_locale: c_int,
    pub coerce_c_locale: c_int,
    pub coerce_c_locale_warn: c_int,
    #[cfg(windows)]
    pub legacy_windows_fs_encoding: c_int,
    pub utf8_mode: c_int,
    pub dev_mode: c_int,
    pub allocator: c_int,
}

/// <https://docs.rs/pyo3/0.16.5/pyo3/ffi/enum._PyStatus_TYPE.html>
#[repr(C)]
#[derive(Copy, Clone, Debug)]
#[allow(non_camel_case_types, clippy::enum_variant_names)]
pub enum _PyStatus_TYPE {
    _PyStatus_TYPE_OK,
    _PyStatus_TYPE_ERROR,
    _PyStatus_TYPE_EXIT,
}

/// <https://docs.python.org/3/c-api/init_config.html#pystatus>
///
/// <https://docs.rs/pyo3/0.16.5/pyo3/ffi/struct.PyStatus.html>
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PyStatus {
    pub _type: _PyStatus_TYPE,
    pub func: *const i8,
    pub err_msg: *const i8,
    pub exitcode: c_int,
}

//noinspection RsUnreachableCode
/// Set utf-8 mode through pre-init
///
/// <https://docs.python.org/3/c-api/init_config.html#preinitialize-python-with-pypreconfig>
unsafe fn pre_init(lib: &Library) -> anyhow::Result<()> {
    trace!("libpython pre-init");
    let py_pre_config_init_python_config: libloading::Symbol<
        unsafe extern "C" fn(*mut PyPreConfig) -> c_void,
    > = lib.get(b"PyPreConfig_InitPythonConfig")?;
    // It's all pretty much the c example code translated to rust
    let mut preconfig: MaybeUninit<PyPreConfig> = MaybeUninit::uninit();
    py_pre_config_init_python_config(preconfig.as_mut_ptr());
    let mut preconfig = preconfig.assume_init();
    // same as PYTHONUTF8=1
    preconfig.utf8_mode = 1;
    trace!("preconfig: {:?}", preconfig);

    let py_pre_initialize: libloading::Symbol<unsafe extern "C" fn(*mut PyPreConfig) -> PyStatus> =
        lib.get(b"Py_PreInitialize")?;
    let py_status_exception: libloading::Symbol<unsafe extern "C" fn(PyStatus) -> c_int> =
        lib.get(b"PyStatus_Exception")?;
    let py_exit_status_exception: libloading::Symbol<unsafe extern "C" fn(PyStatus) -> !> =
        lib.get(b"Py_ExitStatusException")?;

    // This is again from the example
    let status = py_pre_initialize(&mut preconfig as *mut PyPreConfig);
    #[allow(unreachable_code)]
    if py_status_exception(status) != 0 {
        debug!("libpython initialization error: {:?}", status);
        // This should never error, but who knows
        py_exit_status_exception(status);
        // I don't trust cpython
        unreachable!();
    }
    Ok(())
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

/// The way we're using to load symbol by symbol with the type generic is really ugly and cumbersome
/// If you know how to do this with `extern` or even pyo3-ffi directly please tell me.
///
/// sys_executable is the monotrail runner since otherwise we don't get dependencies in
/// subprocesses.
///
/// Returns the exit code from python
#[cfg_attr(test, allow(unreachable_code), allow(unused_variables))]
pub fn inject_and_run_python(
    python_home: &Path,
    python_version: (u8, u8),
    sys_executable: &Path,
    args: &[String],
    finder_data: &FinderData,
) -> anyhow::Result<c_int> {
    trace!(
        "Loading libpython {}.{}",
        python_version.0,
        python_version.1
    );

    #[cfg(test)]
    panic!("Must not load libpython in test");

    let libpython3 = if cfg!(target_os = "windows") {
        // python3.dll doesn't include functions from the limited abi apparently
        python_home.join(format!("python3{}.dll", python_version.1))
    } else if cfg!(target_os = "macos") {
        python_home.join("lib").join(format!(
            "libpython{}.{}.dylib",
            python_version.0, python_version.1
        ))
    } else {
        python_home.join("lib").join("libpython3.so")
    };
    let lib = {
        // platform switch because we need to set RTLD_GLOBAL so extension modules work later
        #[cfg(unix)]
        {
            let flags = libloading::os::unix::RTLD_LAZY | libloading::os::unix::RTLD_GLOBAL;
            let unix_lib = unsafe { libloading::os::unix::Library::open(Some(libpython3), flags)? };
            libloading::Library::from(unix_lib)
        }
        // Entirely untested, but it should at least compile
        #[cfg(windows)]
        unsafe {
            let windows_lib = libloading::os::windows::Library::new(libpython3)
                .context("Failed to load python3y.dll")?;
            libloading::Library::from(windows_lib)
        }
    };
    trace!("Initializing libpython");
    unsafe {
        // initialize python
        // TODO: Do this via python c api instead
        env::set_var("PYTHONNOUSERSITE", "1");

        pre_init(&lib)?;

        trace!("Py_SetPythonHome {}", python_home.display());
        // https://docs.python.org/3/c-api/init.html#c.Py_SetPythonHome
        // void Py_SetPythonHome(const wchar_t *name)
        // Otherwise we get an error that it can't find encoding that tells us to set PYTHONHOME
        let set_python_home: libloading::Symbol<unsafe extern "C" fn(*const wchar_t) -> c_void> =
            lib.get(b"Py_SetPythonHome")?;
        let python_home_wchar_t = WideCString::from_str(python_home.to_string_lossy()).unwrap();
        set_python_home(python_home_wchar_t.as_ptr() as *const wchar_t);

        let sys_executable_str = sys_executable.to_str().context(format!(
            "The path to python must be utf-8, but isn't: {}",
            sys_executable.display()
        ))?;
        if !sys_executable.is_file() {
            bail!(
                "Can't launch python, \
                {} does not exist even though it should have been just created",
                sys_executable_str
            );
        }

        trace!("Py_SetProgramName {}", sys_executable_str);
        // https://docs.python.org/3/c-api/init.html#c.Py_SetProgramName
        // void Py_SetProgramName(const wchar_t *name)
        // To set sys.executable
        let set_program_name: libloading::Symbol<unsafe extern "C" fn(*const wchar_t) -> c_void> =
            lib.get(b"Py_SetProgramName")?;
        let sys_executable = WideCString::from_str(sys_executable_str).unwrap();
        set_program_name(sys_executable.as_ptr() as *const wchar_t);

        trace!("Py_Initialize");
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

        let current_exe_path = current_exe()
            .context("Couldn't determine currently running program 🤨")?
            .parent()
            .context("Currently running program has no parent")?
            .to_string_lossy()
            .to_string();

        let inject_data = InjectData {
            finder_data: finder_data.clone(),
            sys_path_removes: vec![current_exe_path],
            sys_executable: sys_executable_str.to_string(),
        };

        // This is a really horrible way to inject that information and it should be done with
        // PyRun_StringFlags instead
        let inject_data_json = format!(
            "inject_data_str=r'{}'",
            serde_json::to_string(&inject_data)?.replace('\'', r"\u0027")
        );
        let read_json = "inject_data = InjectData.from_json(inject_data_str)";
        let update_and_activate =
            "MonotrailFinder.get_singleton().update_and_activate(inject_data)";
        // First, we inject our Finder class. Next we add the conversion code that's specific to
        // coming from rust without haying pyo3. Third, we serialize the information for the finder
        // as one long json line. We read that data using the python types and
        // deserializer we had injected, then activate it. Finally, we wait connect to the debugger
        // if `PYCHARM_REMOTE_DEBUG` is set with a port.
        // I again wish i knew how to just invoke pyo3 to get this done.
        let command_str = format!(
            "{}\n{}\n{}\n{}\n{}\nmaybe_debug()\n",
            include_str!("../../../python/monotrail/_monotrail_finder.py"),
            include_str!("convert_finder_data.py"),
            // TODO: actual encoding strings
            // This just hopefully works because json uses double quotes so there shouldn't
            // be any escaped single quotes in there
            inject_data_json,
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

/// Allows linking monotrail as python and then doing `python +3.10 -m say.hello`
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
                .with_context(|| format!("Could not parse value of version_major: {}", major))?;
            let minor = minor
                .parse::<u8>()
                .with_context(|| format!("Could not parse value of version_minor: {}", minor))?;
            (major, minor)
        } else {
            bail!("Expect +x.y as first argument (missing dot)");
        };
    Ok(python_version)
}

/// `monotrail run python` implementation. Injects the dependencies and runs the python interpreter
/// with the specified arguments.
pub fn run_python_args(
    args: &[String],
    python_version: Option<&str>,
    root: Option<&Path>,
    extras: &[String],
) -> anyhow::Result<i32> {
    let (args, python_version) = determine_python_version(args, python_version)?;
    let (python_context, python_home) = provision_python(python_version)?;

    let script = if let Some(root) = root {
        Some(root.to_path_buf())
    } else {
        naive_python_arg_parser(&args)
            .map_err(|err| format_err!("Failed to parse python args: {}", err))?
            .map(PathBuf::from)
    };
    debug!("run_python_args: {:?}, `{}`", script, args.join(" "));

    let (specs, scripts, lockfile, project_dir) =
        load_specs(script.as_deref(), extras, &python_context)?;
    let finder_data = install(
        &specs,
        scripts,
        lockfile,
        Some(project_dir),
        &python_context,
    )?;

    run_python_args_finder_data(root, args, &python_context, &python_home, &finder_data)
}

/// `monotrail run python` implementation after installing the requirements
pub fn run_python_args_finder_data(
    root: Option<&Path>,
    args: Vec<String>,
    python_context: &PythonContext,
    python_home: &Path,
    finder_data: &FinderData,
) -> anyhow::Result<i32> {
    let args: Vec<_> = [python_context.sys_executable.to_string_lossy().to_string()]
        .into_iter()
        .chain(args)
        .collect();

    let scripts = find_scripts(
        &finder_data.sprawl_packages,
        Path::new(&finder_data.sprawl_root),
    )
    .context("Failed to collect scripts")?;
    let scripts_tmp = TempDir::new().context("Failed to create tempdir")?;
    let (sys_executable, _) = prepare_execve_environment(
        &scripts,
        &finder_data.root_scripts,
        root,
        scripts_tmp.path(),
        python_context.version,
    )?;
    let exit_code = inject_and_run_python(
        &python_home,
        python_context.version,
        &sys_executable,
        &args,
        &finder_data,
    )
    .context("inject and run failed")?;
    if exit_code != 0 {
        debug!("Python didn't exit with code 0: {}", exit_code);
    }
    Ok(exit_code)
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

/// On unix, we can just symlink to the binary, on windows we need to use a batch file as redirect
fn launcher_indirection(original: impl AsRef<Path>, link: impl AsRef<Path>) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        fs_err::os::unix::fs::symlink(original, link)
            .context("Failed to create symlink for scripts PATH")?;
    }
    #[cfg(windows)]
    {
        // On windows, we can't symlink without being root for an unknown reason
        // (see [std::os::windows::fs::symlink_file]), so instead we create batch files that
        // redirect
        // <https://stackoverflow.com/a/14323360>
        if original.as_ref().display().to_string().contains('"') {
            unimplemented!(
                "path contains quotation marks, escaping for batch scripts needs \
                    to be implemented"
            )
        }
        let batch_script = format!(r#"start /b cmd /c "{}" %*"#, original.as_ref().display());
        fs::write(link, batch_script)
            .context("Failed to create batch script as launcher for python script")?;
    }
    #[cfg(not(any(unix, windows)))]
    compile_error!("launcher_indirection implementation missing");
    Ok(())
}

/// Extends PATH with a directory containing all the scripts we found. This is because many tools
/// such as jupyter depend on scripts being in path instead of using the python api. It also adds
/// monotrail as python so execve with other scripts works (required e.g. by jupyter)
///
/// You have to pass a tempdir to control its lifetime.
///
/// Returns the sys_executable value that is monotrail moonlighting as python and the path
/// with all the scripts and links
pub fn prepare_execve_environment(
    scripts: &BTreeMap<String, PathBuf>,
    root_scripts: &BTreeMap<String, Script>,
    root: Option<&Path>,
    tempdir: &Path,
    python_version: (u8, u8),
) -> anyhow::Result<(PathBuf, PathBuf)> {
    // We could nest that, but there's no point to do that. In normal venv programs also only
    // get one activated set. The only exception would be the poetry lock subprocess but that one
    // we control and don't prepare with this function
    let execve_path_var = format!("{}_EXECVE_PATH", env!("CARGO_PKG_NAME").to_uppercase());
    if let Some(path_dir) = env::var_os(&execve_path_var) {
        debug!(
            "Already an execve environment in {}",
            path_dir.to_string_lossy()
        );
        return Ok((
            Path::new(&path_dir).join("python"),
            PathBuf::from(&path_dir),
        ));
    }
    let path_dir = tempdir.join(format!("{}-scripts-links", env!("CARGO_PKG_NAME")));
    debug!("Preparing execve environment in {}", path_dir.display());
    fs::create_dir_all(&path_dir).context("Failed to create scripts PATH dir")?;
    for (script_name, script_path) in scripts {
        launcher_indirection(script_path, path_dir.join(script_name))?;
    }

    // The scripts of the root project, i.e. those in the current pyproject.toml. Since we don't
    // install the root project, we also didn't install the root scripts and never generated the
    // wrapper scripts. We add them here instead.
    for (script_name, script) in root_scripts {
        let launcher =
            get_script_launcher(&script.module, &script.function, MONOTRAIL_SCRIPT_SHEBANG);
        fs::write(path_dir.join(script_name), &launcher)
            .with_context(|| format!("Failed to write launcher for {}", script_name))?;
    }

    let sys_executable = if cfg!(windows) {
        let python_exe = path_dir.join("python.exe");
        launcher_indirection(current_exe()?, &python_exe)?;
        python_exe
    } else if cfg!(unix) {
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
            launcher_indirection(current_exe()?, path_dir.join(python))?;
        }
        path_dir.join("python")
    } else {
        unreachable!();
    };

    // venv/bin/activate also puts venv scripts first. Our python launcher we have to put first
    // anyway to overwrite system python
    let mut path = path_dir.as_os_str().to_owned();
    if cfg!(windows) {
        path.push(";");
    } else {
        // assumption: non-unix platforms most likely will also use a colon
        path.push(":");
    }
    path.push(env::var_os("PATH").unwrap_or_default());
    env::set_var("PATH", path);

    // Make a execve-spawned monotrail find the configuration we originally read again
    // TODO: Does the subprocess know about the fullpath of the link through which it was called
    //       and can we use that to read those from a file instead which would be more stable
    env::set_var(execve_path_var, &path_dir);
    if let Some(root) = root {
        env::set_var(
            format!("{}_EXECVE_ROOT", env!("CARGO_PKG_NAME").to_uppercase()),
            root,
        );
    }
    env::set_var(
        format!("{}_PYTHON_VERSION", env!("CARGO_PKG_NAME").to_uppercase()),
        format!("{}.{}", python_version.0, python_version.1),
    );

    Ok((sys_executable, path_dir))
}

#[cfg(test)]
mod tests {
    use crate::inject_and_run::naive_python_arg_parser;
    use crate::run_python_args;
    use crate::utils::cache_dir;
    use anyhow::Context;
    use fs_err as fs;
    use std::fs::File;
    use std::path::Path;

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

    #[test]
    fn no_deps_specs_file() {
        // Fake already installed python
        let python_parent_dir = cache_dir().unwrap().join("python-build-standalone");
        let unpack_dir = python_parent_dir.join(format!("cpython-{}.{}", 3, 141));
        let install_dir = unpack_dir.join("python").join("install");
        let libpython3 = if cfg!(target_os = "windows") {
            // python3.dll doesn't include functions from the limited abi apparently
            install_dir.join("python3141.dll")
        } else if cfg!(target_os = "macos") {
            install_dir.join("lib").join("libpython3.141.dylib")
        } else {
            install_dir.join("lib").join("libpython3.so")
        };

        // Make it assume it's installed
        fs::create_dir_all(libpython3.parent().unwrap()).unwrap();
        File::create(libpython3).unwrap();

        // Make python runnable for the PEP508 markers
        let bin_dir = if cfg!(windows) {
            unpack_dir.join("python").join("install")
        } else {
            unpack_dir.join("python").join("install").join("bin")
        };
        fs::create_dir_all(&bin_dir).unwrap();
        let bin = if cfg!(windows) {
            bin_dir.join("python.exe")
        } else {
            bin_dir.join("python3")
        };
        if !bin.is_file() {
            #[cfg(unix)]
            {
                let python3 = which::which("python3").unwrap();
                fs_err::os::unix::fs::symlink(python3, bin)
                    .context("Failed to create symlink for scripts PATH")
                    .unwrap();
            }
            #[cfg(windows)]
            {
                // python3 opens the windows store, even with python installed 🙄
                let python = which::which("python").unwrap();
                // symlink are not allowed for normal users on windows
                fs_err::hard_link(python, bin)
                    .context("Failed to create symlink for scripts PATH")
                    .unwrap();
            }
        }

        let err = run_python_args(&[], Some("3.141"), Some(Path::new("/")), &[]).unwrap_err();
        let errors = err.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        assert_eq!(errors, ["neither pyproject.toml nor requirements.txt not found next to / nor in any parent directory"]);
    }
}
