#!/bin/bash
set -e

#pip download -d test-data/popular-wheels -r popular.txt
cargo build --release
cargo test --release
pytest -s test_binary
