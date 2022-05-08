#!/bin/bash

set -e

# cd to project root
cd "$(git rev-parse --show-toplevel)"
rm -f target-maturin/wheels/install_wheel_rs-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip --no-sdist -i python --cargo-extra-args="--features=python_bindings" -m install-wheel-rs/Cargo.toml
.venv/bin/pip install target-maturin/wheels/install_wheel_rs-*.whl
.venv/bin/pytest install-wheel-rs/test
