#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip --no-sdist -i python --cargo-extra-args="--features=python_bindings"
virtualenv -p 3.8 -q .venv
.venv/bin/pip uninstall -y -q monotrail
.venv/bin/pip install -q target-maturin/wheels/monotrail-*.whl

#CARGO_TARGET_DIR=target-maturin maturin build --strip -i python --cargo-extra-args="--features=python_bindings"
#CARGO_TARGET_DIR=target-maturin VIRTUAL_ENV=$(pwd)/.venv maturin develop --strip --cargo-extra-args="--features=python_bindings"
# Try to get the develop import first
# this currently clashes with the jupyter setup
#rm -rf .venv

echo "Installed"
time .venv/bin/monotrail_python data_science_project/import_pandas.py
time .venv/bin/monotrail_python data_science_project/make_paper.py
time MONOTRAIL_CWD="data_science_project" .venv/bin/monotrail_script numpy_identity_3
# .venv/bin/monotrail_python flipstring/flip.py "hello world!"
#.venv/bin/pytest test/python