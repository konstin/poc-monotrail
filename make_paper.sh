#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip
virtualenv -p 3.8 .venv
.venv/bin/pip uninstall -y monotrail
.venv/bin/pip install target-maturin/wheels/monotrail-*.whl

#CARGO_TARGET_DIR=target-maturin maturin build --strip
#CARGO_TARGET_DIR=target-maturin VIRTUAL_ENV=$(pwd)/.venv maturin develop --strip
# Try to get the develop import first
# this currently clashes with the jupyter setup
#rm -rf .venv

echo "Installed"
.venv/bin/monotrail_python data_science_project/import_pandas.py
.venv/bin/monotrail_python data_science_project/make_paper.py
MONOTRAIL_CWD="data_science_project" .venv/bin/monotrail_script numpy_identity_3
# .venv/bin/monotrail_python flipstring/flip.py "hello world!"
# .venv/bin/pytest test/python