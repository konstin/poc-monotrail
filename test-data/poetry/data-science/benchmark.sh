#!/usr/bin/env bash

set -e

cd "$(dirname "$0")"

BENCHMARK_OPTIONS="-E tqdm_feature"
# shellcheck disable=SC2086
poetry export -q --without-hashes -o requirements-benchmark.txt $BENCHMARK_OPTIONS

pip --version
poetry --version
# Something with relative/absolute paths is broken
(
  cd ../../..
  cargo build -q --release --bin monotrail
  cd -
)

# pip benchmark
rm -rf .venv && virtualenv -q .venv
time .venv/bin/pip install -q -r requirements-benchmark.txt
# real    0m19,168s   user    0m15,533s   sys     0m2,114s

# poetry benchmark
rm -rf .venv && virtualenv -q .venv
# shellcheck disable=SC2086
time VIRTUAL_ENV=$(pwd)/.venv PATH="$PATH:$(pwd)/.venv/bin" poetry install -q --no-root $BENCHMARK_OPTIONS
# real    0m16,924s   user    0m38,731s   sys     0m5,372s

# monotrail benchmark
rm -rf .venv && virtualenv -q .venv
# shellcheck disable=SC2086
time VIRTUAL_ENV=$(pwd)/.venv PATH="$PATH:$(pwd)/.venv/bin" ../../../target/x86_64-unknown-linux-musl/release/monotrail poetry-install $BENCHMARK_OPTIONS
# real    0m5,414s    user    0m13,421s   sys     0m2,300s
