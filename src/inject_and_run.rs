use crate::monotrail::install_specs_to_finder;
use crate::standalone_python::provision_python;
use crate::{get_specs, Pep508Environment};
use anyhow::{bail, format_err, Context};
use libc::{c_int, c_void, wchar_t};
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use widestring::WideCString;

/// python has idiosyncratic cli options that are hard to replicate with clap, so we roll our own
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
    python_root: &Path,
    args: &[String],
    finder_data: &str,
) -> anyhow::Result<c_int> {
    debug!("Loading libpython");
    let libpython3_so = python_root
        .join("install")
        .join("lib")
        .join("libpython3.so");
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
    unsafe {
        debug!("Initializing python");
        // initialize python
        // otherwise we get an error that it can't find encoding that tells us to set PYTHONHOME
        env::set_var("PYTHONHOME", python_root.join("install"));
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
            "{}\n{}\nfinder_data_str=r'{}'\n{}\n{}\n",
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

        info!("Running main: {:?}", args);
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
pub fn parse_plus_arg(python_args: Vec<String>) -> anyhow::Result<(Vec<String>, Option<(u8, u8)>)> {
    if let Some(first_arg) = python_args.get(0) {
        if first_arg.starts_with('+') {
            if let Some((major, minor)) = first_arg.trim_start_matches('+').split_once('.') {
                let major = major
                    .parse::<u8>()
                    .context("Could not parse value of version_major")?;
                let minor = minor
                    .parse::<u8>()
                    .context("Could not parse value of version_minor")?;
                return Ok((python_args[1..].to_vec(), Some((major, minor))));
            } else {
                bail!("Expect +x.y as first argument (missing dot)");
            }
        }
    }
    Ok((python_args, None))
}

pub fn run_from_python_args(python_args: Vec<String>) -> anyhow::Result<()> {
    let (args, python_version) = parse_plus_arg(python_args)?;
    let python_version = python_version.unwrap_or((3, 8));
    let script = naive_python_arg_parser(&args)
        .map_err(|err| format_err!("Failed to parse python args: {}", err))?;
    debug!("monotrail_from_env script: {:?}", script);

    let python_root = provision_python(python_version).unwrap();
    let python_binary = python_root.join("install").join("bin").join("python3");

    let pep508_env = Pep508Environment::from_python(&python_binary);

    let (specs, scripts, lockfile) = get_specs(
        script.map(PathBuf::from).as_deref(),
        &[],
        &python_binary,
        python_version,
        &pep508_env,
        Some(python_root.clone()),
    )?;

    let finder_data = install_specs_to_finder(
        &specs,
        python_binary.to_string_lossy().to_string(),
        python_version,
        scripts,
        lockfile,
        None,
    )?;

    let args: Vec<_> = [python_binary.to_string_lossy().to_string()]
        .into_iter()
        .chain(args)
        .collect();

    println!(
        "Done: {:?}",
        inject_and_run_python(
            &python_root,
            &args,
            &serde_json::to_string(&finder_data).unwrap()
        )
    );
    Ok(())
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
