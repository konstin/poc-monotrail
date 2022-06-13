#!/bin/bash
set -e

rm -f target-maturin/wheels/monotrail-*.whl
CARGO_TARGET_DIR=target-maturin maturin build --release --strip
virtualenv -q .venv
.venv/bin/pip uninstall -y -q monotrail
.venv/bin/pip install -q target-maturin/wheels/monotrail-*.whl

# Run pytest, entrypoint
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 MONOTRAIL_EXTRAS="import-json" ../monotrail/.venv/bin/monotrail_script pytest)
# Run pytest, module
(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 MONOTRAIL_EXTRAS="import-json" ../monotrail/.venv/bin/monotrail_python -m pytest)
# Test interactive console
(cd ../meine-stadt-transparent && ../monotrail/.venv/bin/monotrail_python -c "import django; print('hi django ' + django.__version__)")
# Test manage.py script
(cd ../meine-stadt-transparent && ../monotrail/.venv/bin/monotrail_python manage.py | wc -l)
(cd ../meine-stadt-transparent && ../monotrail/target/debug/monotrail run --extras import-json python ./manage.py | wc -l)
#(cd ../meine-stadt-transparent && MONOTRAIL_EXTRAS="import-json" ../monotrail/.venv/bin/monotrail_python manage.py runserver)
#(cd ../meine-stadt-transparent && SKIP_SLOW_TESTS=1 ../monotrail/target/debug/monotrail run --extras import-json script pytest)
