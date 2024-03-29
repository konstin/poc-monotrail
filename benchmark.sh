#!/usr/bin/env bash

set -e

BENCHMARK_DIR=test-data/poetry/ibis
BENCHMARK_OPTIONS="-E all"
#BENCHMARK_DIR=test-data/poetry/black
#BENCHMARK_OPTIONS=""
#BENCHMARK_DIR=test-data/poetry/data-science
#BENCHMARK_OPTIONS="-E tqdm_feature"
#BENCHMARK_DIR=test-data/poetry/mst
#BENCHMARK_OPTIONS="-E import-json"

echo "$BENCHMARK_DIR $BENCHMARK_OPTIONS"
pip --version
poetry --version
cargo build -q --release --bin monotrail

ROOT=$(pwd)
cd "$(dirname "$0")/$BENCHMARK_DIR"

# shellcheck disable=SC2086
poetry export -q --without-hashes -o requirements-benchmark.txt $BENCHMARK_OPTIONS

VIRTUAL_ENV=$(pwd)/.venv PATH="../../../target/release/:$(pwd)/.venv/bin:$PATH" hyperfine \
  --prepare "virtualenv --clear -q .venv" \
  --runs 100 \
  --warmup 1 \
  --export-json hyperfine.json \
  --export-markdown hyperfine.md \
  "$ROOT/target/release/monotrail poetry-install --no-compile $BENCHMARK_OPTIONS" \
  ".venv/bin/pip install --no-compile -q -r requirements-benchmark.txt" \
  "poetry install -q --no-root --only main $BENCHMARK_OPTIONS" \
  # ".venv/bin/pip install -q -r requirements-benchmark.txt" \
  # "poetry install --compile -q --no-root --only main $BENCHMARK_OPTIONS" \
  # "/home/konsti/monotrail/target/release/monotrail poetry-install $BENCHMARK_OPTIONS" \
