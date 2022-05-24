use anyhow::Context;
use clap::Parser;
use monotrail::{run_cli, run_python_args, Cli};
use std::env;
use std::env::args;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Checks under what name we're running and if it's python, shortcuts to running as python,
/// otherwise does the normal cli run
fn run() -> anyhow::Result<Option<i32>> {
    // Notably, we can't use env::current_exe() here because it resolves the symlink
    let args: Vec<String> = args().into_iter().collect();
    let name = Path::new(
        args.first()
            .context("No first argument, this should always be set 🤨")?,
    )
    .file_name()
    .context("Expected first argument to have a filename")?
    .to_string_lossy()
    .to_string();
    if name.starts_with("python") {
        debug!("START: Running as python: {:?}", args);
        // TODO: Also keep the extras
        let root = env::var_os("MONOTRAIL_EXECVE_ROOT").map(PathBuf::from);
        // TODO: Make sure we also keep the python version
        Ok(Some(run_python_args(
            &args[1..],
            None,
            root.as_deref(),
            &[],
        )?))
    } else {
        debug!("START: Running as monotrail: '{}' {:?}", name, args);
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
        Ok(Some(exit_code)) => {
            std::process::exit(exit_code);
        }
    }
}
