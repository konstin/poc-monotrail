#!/bin/bash
set -e

#pip download -d popular-wheels -r popular.txt
cargo build --release
cargo test --release
pytest test_binary
