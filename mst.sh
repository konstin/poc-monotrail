#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip --no-sdist -i python --cargo-extra-args="--features=python_bindings"
virtualenv -q .venv
rm -f .venv/lib/python3.*/site-packages/load_monotrail.pth
.venv/bin/pip uninstall -y -q monotrail
.venv/bin/pip install -q target-maturin/wheels/monotrail-*.whl
cp monotrail.data/data/load_monotrail.pth .venv/lib/python3.*/site-packages/

# Run pytest, entrypoint
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 MONOTRAIL_EXTRAS="import-json" ../monotrail/.venv/bin/python -m monotrail.run pytest)
# Run pytest, module
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 MONOTRAIL=1 MONOTRAIL_EXTRAS="import-json" ../monotrail/.venv/bin/python -m pytest)
# Test interactive console
(cd ../meine-stadt-transparent && MONOTRAIL=1 ../monotrail/.venv/bin/python -c "import django; print('hi django ' + django.__version__)")
# Test manage.py script
MONOTRAIL=1 ENV_PATH=../meine-stadt-transparent/.env .venv/bin/python ../meine-stadt-transparent/manage.py | wc -l
#(cd ../meine-stadt-transparent && MONOTRAIL=1 MONOTRAIL_EXTRAS="import-json" ../monotrail/.venv/bin/python manage.py runserver)
