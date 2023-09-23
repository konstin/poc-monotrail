#![allow(clippy::needless_borrow)]

use anyhow::Context;
use clap::Parser;
use monotrail::{parse_major_minor, run_cli, run_python_args, Cli};
use std::env;
use std::env::args;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Checks under what name we're running and if it's python, shortcuts to running as python,
/// otherwise does the normal cli run
fn run() -> anyhow::Result<Option<i32>> {
    // Notably, we can't use env::current_exe() here because it resolves the symlink
    let args: Vec<String> = args().collect();
    let filename = Path::new(
        args.first()
            .context("No first argument, this should always be set 🤨")?,
    )
    .file_name()
    .context("Expected first argument to have a filename")?;
    let name = filename
        .to_str()
        .with_context(|| format!("First argument filename isn't utf-8: {:?}", filename))?
        .to_string();
    if let Some(version) = name.strip_prefix("python") {
        let root = env::var_os(format!(
            "{}_EXECVE_ROOT",
            env!("CARGO_PKG_NAME").to_uppercase()
        ))
        .map(PathBuf::from);
        debug!(
            "START: python as {}: `{}` in {:?}",
            name,
            args.join(" "),
            root
        );
        // TODO: Also keep the extras
        // Allows to link monotrail to .local/bin/python3.10 and use it as python from the terminal
        let python_version = if version.is_empty() || version == "3" {
            None
        } else {
            parse_major_minor(&version).with_context(|| {
                format!(
                    "Can't launch as {}, couldn't parse {} as `python`, `python3` or `pythonx.y`",
                    name, version
                )
            })?;
            Some(version)
        };
        Ok(Some(run_python_args(
            &args[1..],
            python_version,
            root.as_deref(),
            &[],
        )?))
    } else {
        debug!("START: monotrail as '{}': `{}`", name, args.join(" "));
        run_cli(Cli::parse(), None)
    }
}

fn main() {
    // Good enough for now
    if env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt::init();
    } else {
        let format = tracing_subscriber::fmt::format()
            .with_level(false)
            .with_target(false)
            .without_time()
            .compact();
        tracing_subscriber::fmt().event_format(format).init();
    }

    match run() {
        Err(e) => {
            eprintln!("💥 {} failed", env!("CARGO_PKG_NAME"));
            for cause in e.chain().collect::<Vec<_>>().iter() {
                eprintln!("  Caused by: {}", cause);
            }
            std::process::exit(1);
        }
        Ok(None) => {}
        // If python gave us an exit code, return that to the user
        Ok(Some(exit_code)) => {
            debug!("Exit code: {}", exit_code);
            std::process::exit(exit_code);
        }
    }
}
