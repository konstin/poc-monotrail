#!/usr/bin/env bash

rm -rf test-venvs/benchmark
virtualenv test-venvs/benchmark
VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_plotly.svg -- target/release/monotrail wheel-install --no-compile test-data/popular-wheels/plotly-5.5.0-py2.py3-none-any.whl
VIRTUAL_ENV=test-venvs/benchmark flamegraph -o flamegraph_popular.svg -- target/release/monotrail wheel-install --no-compile test-data/popular-wheels/*
