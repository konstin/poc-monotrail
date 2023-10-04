#!/usr/bin/env bash

cargo build -p install-wheel-rs --bin install-wheel-rs --profile profiling --no-default-features --features cli
rm -rf test-venvs/benchmark
virtualenv -p 3.8 test-venvs/benchmark

VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_plotly.svg -- target/profiling/install-wheel-rs --major 3 --minor 8 test-data/popular-wheels/plotly-5.5.0-py2.py3-none-any.whl
VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_popular.svg -- target/profiling/install-wheel-rs --major 3 --minor 8 test-data/popular-wheels/*
