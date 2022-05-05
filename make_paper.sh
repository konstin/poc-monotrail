#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
#CARGO_TARGET_DIR=target-maturin VIRTUAL_ENV=$(pwd)/.venv maturin develop --release --strip --cargo-extra-args="--features=python_bindings"
# Try to get the develop import first
#cp load_monotrail.pth .venv/lib/python3.*/site-packages/z_load_monotrail.pth
zip -ur target-maturin/wheels/monotrail-*.whl load_monotrail.pth
# this currently clashes with the jupyter setup
#rm -rf .venv
virtualenv -q .venv
.venv/bin/pip uninstall -y -q monotrail
.venv/bin/pip install -q target-maturin/wheels/monotrail-*.whl
MONOTRAIL=1 .venv/bin/python data_science_project/import_pandas.py
MONOTRAIL=1 .venv/bin/python data_science_project/make_paper.py
MONOTRAIL=1 .venv/bin/python flipstring/flip.py "hello world!"
