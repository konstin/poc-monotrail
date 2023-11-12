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
