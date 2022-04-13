#!/bin/bash
set -e

maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
zip -ur target/wheels/virtual_sprawl-0.1.0-cp37-abi3-manylinux_2_31_x86_64.whl load_virtual_sprawl.pth
.venv/bin/pip uninstall -y -q virtual-sprawl
.venv/bin/pip install -q target/wheels/virtual_sprawl-0.1.0-cp37-abi3-manylinux_2_31_x86_64.whl
VIRTUAL_SPRAWL=1 ENV_PATH=../.env .venv/bin/python ../meine-stadt-transparent/manage.py runserver
