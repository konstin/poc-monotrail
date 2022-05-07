#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip --no-sdist -i python --cargo-extra-args="--features=python_bindings"
virtualenv -q .venv
rm -f .venv/lib/python3.*/site-packages/load_monotrail.pth
.venv/bin/pip uninstall -y -q monotrail
.venv/bin/pip install -q target-maturin/wheels/monotrail-*.whl
cp load_monotrail.pth .venv/lib/python3.*/site-packages/

#CARGO_TARGET_DIR=target-maturin maturin build --strip -i python --cargo-extra-args="--features=python_bindings"
#CARGO_TARGET_DIR=target-maturin VIRTUAL_ENV=$(pwd)/.venv maturin develop --strip --cargo-extra-args="--features=python_bindings"
# Try to get the develop import first
#zip -ur target-maturin/wheels/monotrail-*.whl load_monotrail.pth
# this currently clashes with the jupyter setup
#rm -rf .venv

echo "Installed"
MONOTRAIL=1 .venv/bin/python data_science_project/import_pandas.py
MONOTRAIL=1 .venv/bin/python data_science_project/make_paper.py
# MONOTRAIL=1 .venv/bin/python flipstring/flip.py "hello world!"
# .venv/bin/pytest test_python