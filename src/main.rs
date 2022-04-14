use anyhow::{bail, Context, Result};
use clap::Parser;
use std::env;
use std::path::{Path, PathBuf};
use tracing::debug;
use virtual_sprawl::{run, Cli};

fn cli() -> Result<()> {
    let cli = Cli::parse();
    debug!("VIRTUAL_ENV: {:?}", env::var_os("VIRTUAL_ENV"));
    let venv = if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
        PathBuf::from(virtual_env)
    } else if let Cli::PoetryInstall { pyproject_toml, .. } = &cli {
        let venv = pyproject_toml
            .as_deref()
            .unwrap_or_else(|| Path::new("pyproject.toml"))
            .parent()
            .context("Invalid pyproject.toml path")?
            .join(".venv");
        if venv.join("pyvenv.cfg").is_file() {
            venv
        } else {
            bail!("No venv active or next to lockfile");
        }
    } else {
        bail!("Will only install in a virtualenv");
    };

    run(cli, &venv)
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

    if let Err(e) = cli() {
        eprintln!("💥 {} failed", env!("CARGO_PKG_NAME"));
        for cause in e.chain().collect::<Vec<_>>().iter() {
            eprintln!("  Caused by: {}", cause);
        }
        std::process::exit(1);
    }
}
