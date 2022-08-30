#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin2 maturin build --release --strip
virtualenv -p 3.8 .venv
.venv/bin/pip install --force-reinstall target-maturin/wheels/monotrail-*.whl

echo "Installed"
.venv/bin/monotrail_python data_science_project/import_pandas.py
.venv/bin/monotrail_python data_science_project/make_paper.py
MONOTRAIL_CWD="data_science_project" .venv/bin/monotrail_script numpy_identity_3
# .venv/bin/monotrail_python flipstring/flip.py "hello world!"
# .venv/bin/pytest test/python
