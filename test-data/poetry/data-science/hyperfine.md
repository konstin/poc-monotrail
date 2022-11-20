| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install --no-compile -q -r requirements-benchmark.txt` | 6.238 ± 0.172 | 6.070 | 6.413 | 3.01 ± 0.34 |
| `poetry install -q --no-root --only main -E tqdm_feature` | 10.266 ± 0.008 | 10.259 | 10.275 | 4.96 ± 0.54 |
| `poetry@installer install -q --no-root --only main -E tqdm_feature` | 3.560 ± 0.081 | 3.495 | 3.650 | 1.72 ± 0.19 |
| `/home/konsti/monotrail/target/release/monotrail poetry-install -E tqdm_feature` | 5.648 ± 1.659 | 3.872 | 7.157 | 2.73 ± 0.85 |
| `/home/konsti/monotrail/target/release/monotrail poetry-install --no-compile -E tqdm_feature` | 2.070 ± 0.226 | 1.935 | 2.330 | 1.00 |
