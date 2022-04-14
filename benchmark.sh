#!/usr/bin/env bash

set -e

BENCHMARK_DIR=test-data/poetry/data-science
BENCHMARK_OPTIONS="-E tqdm_feature"
#BENCHMARK_DIR=test-data/poetry/mst
#BENCHMARK_OPTIONS="-E import-json"

pip --version
poetry --version
cargo build -q --release --target x86_64-unknown-linux-musl --bin virtual-sprawl

cd "$(dirname "$0")/$BENCHMARK_DIR"

# shellcheck disable=SC2086
poetry export -q --without-hashes -o requirements-benchmark.txt $BENCHMARK_OPTIONS

VIRTUAL_ENV=$(pwd)/.venv PATH="../../../target/x86_64-unknown-linux-musl/release/:$(pwd)/.venv/bin:$PATH" hyperfine \
  --prepare "rm -rf .venv && virtualenv -q .venv" \
  --runs 3 \
  --export-json hyperfine.json \
  --export-markdown hyperfine.md \
  ".venv/bin/pip install -q -r requirements-benchmark.txt" \
  "poetry install -q --no-root $BENCHMARK_OPTIONS" \
  "virtual-sprawl poetry-install $BENCHMARK_OPTIONS"
