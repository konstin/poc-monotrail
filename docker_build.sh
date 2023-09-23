#!/bin/bash

set -ex

docker run --rm -v "$(pwd):/io" -e CARGO_TARGET_DIR=target-docker ghcr.io/pyo3/maturin build --release --strip -m crates/monotrail/Cargo.toml
