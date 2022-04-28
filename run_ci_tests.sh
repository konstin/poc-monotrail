#!/bin/bash
set -e

#pip download -d popular-wheels -r popular.txt
cargo build --release --target x86_64-unknown-linux-musl
cargo test --release --target x86_64-unknown-linux-musl
pytest