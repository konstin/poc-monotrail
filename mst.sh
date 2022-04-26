#!/bin/bash
set -e

maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
zip -ur target/wheels/virtual_sprawl-*.whl load_virtual_sprawl.pth
.venv/bin/pip uninstall -y -q virtual-sprawl
.venv/bin/pip install -q target/wheels/virtual_sprawl-*.whl
# Run pytest, entrypoint
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 VIRTUAL_SPRAWL=1 VIRTUAL_SPRAWL_EXTRAS="import-json" ../virtual_sprawl/.venv/bin/python -m virtual_sprawl.run pytest)
# Run pytest, module
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 VIRTUAL_SPRAWL=1 VIRTUAL_SPRAWL_EXTRAS="import-json" ../virtual_sprawl/.venv/bin/python -m pytest)
# Test interactive console
(cd ../meine-stadt-transparent && VIRTUAL_SPRAWL=1 ../virtual_sprawl/.venv/bin/python -c "print('hi')")
# Test manage.py script
VIRTUAL_SPRAWL=1 ENV_PATH=../meine-stadt-transparent/.env .venv/bin/python ../meine-stadt-transparent/manage.py | wc -l
#(cd ../meine-stadt-transparent && VIRTUAL_SPRAWL=1 VIRTUAL_SPRAWL_EXTRAS="import-json" ../virtual_sprawl/.venv/bin/python manage.py runserver)
