use clap::Parser;
use monotrail::{run, Cli};
use std::env;

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

    if let Err(e) = run(Cli::parse(), None) {
        eprintln!("💥 {} failed", env!("CARGO_PKG_NAME"));
        for cause in e.chain().collect::<Vec<_>>().iter() {
            eprintln!("  Caused by: {}", cause);
        }
        std::process::exit(1);
    }
}
