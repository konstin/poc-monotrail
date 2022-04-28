#!/bin/bash
set -e

CARGO_TARGET_DIR=target-maturin maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
zip -ur target-maturin/wheels/monorail-*.whl load_monorail.pth
rm -rf .venv
virtualenv .venv
.venv/bin/pip install -q target-maturin/wheels/monorail-*.whl
MONORAIL=1 .venv/bin/python data_science_project/import_pandas.py
MONORAIL=1 .venv/bin/python data_science_project/make_paper.py
MONORAIL=1 .venv/bin/python flipstring/flip.py "hello world!"
