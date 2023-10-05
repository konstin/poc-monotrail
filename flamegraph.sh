#!/usr/bin/env bash

set -e

cargo build -p install-wheel-rs --bin install-wheel-rs --profile profiling --no-default-features --features cli
rm -rf test-venvs/benchmark
virtualenv -p 3.8 test-venvs/benchmark

compressed=test-data/popular-wheels/plotly-5.5.0-py2.py3-none-any.whl
stored=test-data/stored/plotly-5.5.0-py2.py3-none-any.whl

if [ ! -f "$stored" ]; then
  mkdir -p test-data/stored
  # https://stackoverflow.com/a/34701207/3549270
  tmp_dir=$(mktemp -d)
  unzip -qq "$compressed" -d "$tmp_dir"
  pwd=$(pwd)
  cd "$tmp_dir"
  zip -q -0 -r "$pwd/$stored" ./*
  cd "$pwd"
  rm -r "$tmp_dir"
fi


VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_plotly.svg -- target/profiling/install-wheel-rs --skip-hashes --major 3 --minor 8 "$compressed"
VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_plotly_stored.svg -- target/profiling/install-wheel-rs --skip-hashes --major 3 --minor 8 "$stored"
VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_popular.svg -- target/profiling/install-wheel-rs --skip-hashes --major 3 --minor 8 test-data/popular-wheels/*
