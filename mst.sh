#!/bin/bash
set -e

maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
zip -ur target/wheels/virtual_sprawl-0.1.0-cp37-abi3-manylinux_2_31_x86_64.whl load_virtual_sprawl.pth
.venv/bin/pip uninstall -y -q virtual-sprawl
.venv/bin/pip install -q target/wheels/virtual_sprawl-0.1.0-cp37-abi3-manylinux_2_31_x86_64.whl
# Run pytest, entrypoint
(cd ../meine-stadt-transparent; SKIP_SLOW_TESTS=1 VIRTUAL_SPRAWL=1 ../virtual_sprawl/.venv/bin/python -m virtual_sprawl.run pytest -p pytest_django)
# Run pytest, module
(cd ../meine-stadt-transparent; SKIP_SLOW_TESTS=1 VIRTUAL_SPRAWL=1 .venv/bin/python -m pytest)
# Test interactive console
(cd ../meine-stadt-transparent; VIRTUAL_SPRAWL=1 .venv/bin/python -c "print('hi')")
# Test manage.py script
VIRTUAL_SPRAWL=1 ENV_PATH=../meine-stadt-transparent/.env .venv/bin/python ../meine-stadt-transparent/manage.py
(cd ../meine-stadt-transparent; VIRTUAL_SPRAWL=1 VIRTUAL_SPRAWL_EXTRAS="import-json" ../virtual_sprawl/.venv/bin/python manage.py runserver)
