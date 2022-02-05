use anyhow::{bail, Result};
use clap::Parser;
use install_wheel_rs::{run, Cli};
use std::env;
use std::path::PathBuf;
use tracing::debug;

fn cli() -> Result<()> {
    debug!("VIRTUAL_ENV: {:?}", env::var_os("VIRTUAL_ENV"));
    let venv = if let Some(virtual_env) = env::var_os("VIRTUAL_ENV") {
        PathBuf::from(virtual_env)
    } else {
        bail!("Will only install in a virtualenv");
    };

    run(Cli::parse(), &venv)
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
