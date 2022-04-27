#!/bin/bash
set -e

maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
zip -ur target/wheels/monorail-*.whl load_monorail.pth
.venv/bin/pip uninstall -y -q monorail
.venv/bin/pip install -q target/wheels/monorail-*.whl
MONORAIL=1 .venv/bin/python data_science_project/import_pandas.py
MONORAIL=1 .venv/bin/python data_science_project/make_paper.py
MONORAIL=1 .venv/bin/python flipstring/flip.py "hello world!"
