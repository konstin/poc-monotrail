| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install -q -r requirements-benchmark.txt` | 12.302 ± 0.557 | 11.688 | 12.773 | 8.52 ± 0.75 |
| `.venv/bin/pip install --no-compile -q -r requirements-benchmark.txt` | 6.578 ± 0.063 | 6.522 | 6.646 | 4.56 ± 0.35 |
| `poetry install --compile -q --no-root --only main -E tqdm_feature` | 10.606 ± 0.584 | 10.222 | 11.278 | 7.35 ± 0.69 |
| `poetry install -q --no-root --only main -E tqdm_feature` | 4.180 ± 0.020 | 4.166 | 4.203 | 2.89 ± 0.22 |
| `/home/konsti/monotrail/target/release/monotrail poetry-install -E tqdm_feature` | 5.447 ± 0.192 | 5.284 | 5.658 | 3.77 ± 0.31 |
| `/home/konsti/monotrail/target/release/monotrail poetry-install --no-compile -E tqdm_feature` | 1.444 ± 0.109 | 1.344 | 1.560 | 1.00 |
