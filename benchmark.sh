#!/usr/bin/env bash

set -e

BENCHMARK_DIR=test-data/poetry/data-science
BENCHMARK_OPTIONS="-E tqdm_feature"

pip --version
poetry --version
cargo build -q --release --target x86_64-unknown-linux-musl --bin virtual-sprawl

cd "$(dirname "$0")/$BENCHMARK_DIR"

# shellcheck disable=SC2086
poetry export -q --without-hashes -o requirements-benchmark.txt $BENCHMARK_OPTIONS

VIRTUAL_ENV=$(pwd)/.venv PATH="$PATH:$(pwd)/.venv/bin" hyperfine \
  --prepare "rm -rf .venv && virtualenv -q .venv" \
  --runs 3 \
  --export-json hyperfine.json \
  '.venv/bin/pip install -q -r requirements-benchmark.txt' \
  'poetry install -q --no-root $BENCHMARK_OPTIONS' \
  '../../../target/x86_64-unknown-linux-musl/release/virtual-sprawl poetry-install $BENCHMARK_OPTIONS'
