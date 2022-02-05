#!/bin/bash
virtualenv .venv-benchmark
VIRTUAL_ENV=.venv-benchmark hyperfine -r 5 -p ".venv-benchmark/bin/pip uninstall -y plotly" \
  "target/release/install-wheel-rs install-files --no-compile wheels/plotly-5.5.0-py2.py3-none-any.whl" \
  ".venv-benchmark/bin/pip install --no-compile --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl"
rm -r .venv-benchmark