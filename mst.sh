#!/bin/bash
set -e

maturin build --release --strip -i python --cargo-extra-args="--features=python_bindings"
# VIRTUAL_ENV=/home/konsti/monorail/.venv maturin develop --release --strip --cargo-extra-args="--features=python_bindings"
zip -ur target/wheels/monorail-*.whl load_monorail.pth
.venv/bin/pip uninstall -y -q monorail
.venv/bin/pip install -q target/wheels/monorail-*.whl
# Run pytest, entrypoint
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 MONORAIL=1 MONORAIL_EXTRAS="import-json" ../monorail/.venv/bin/python -m monorail.run pytest)
# Run pytest, module
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 MONORAIL=1 MONORAIL_EXTRAS="import-json" ../monorail/.venv/bin/python -m pytest)
# Test interactive console
(cd ../meine-stadt-transparent && MONORAIL=1 ../monorail/.venv/bin/python -c "import django; print('hi django ' + django.__version__)")
# Test manage.py script
MONORAIL=1 ENV_PATH=../meine-stadt-transparent/.env .venv/bin/python ../meine-stadt-transparent/manage.py | wc -l
#(cd ../meine-stadt-transparent && MONORAIL=1 MONORAIL_EXTRAS="import-json" ../monorail/.venv/bin/python manage.py runserver)
