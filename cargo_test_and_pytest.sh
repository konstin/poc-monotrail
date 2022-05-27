#!/bin/bash
set -e

#pip download -d test-data/popular-wheels -r test-data/popular.txt
cargo build --release
cargo test --release
.venv/bin/pytest test/binary
