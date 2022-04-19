#!/bin/bash
set -e

#pip download -d popular-wheels -r popular.txt
#cargo build --release
#cargo test --release
cargo build
cargo test
python -m test.compare popular-wheels/tqdm-4.62.3-py2.py3-none-any.whl
python -m test.test_tqdm
python -m test.compare test-data/wheels/purelib_and_platlib-1.0.0-cp38-cp38-linux_x86_64.whl
python -m test.test_piptests
python -m test.test_popular
python -m test.compare_poetry data_science_project -E tqdm_feature
