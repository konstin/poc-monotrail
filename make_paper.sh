#!/bin/bash
set -e

maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
zip -ur target/wheels/virtual_sprawl-*.whl load_virtual_sprawl.pth
.venv/bin/pip uninstall -y -q virtual-sprawl
.venv/bin/pip install -q target/wheels/virtual_sprawl-*.whl
VIRTUAL_SPRAWL=1 .venv/bin/python data_science_project/import_pandas.py
VIRTUAL_SPRAWL=1 .venv/bin/python data_science_project/make_paper.py
VIRTUAL_SPRAWL=1 .venv/bin/python flipstring/flip.py "hello world!"
