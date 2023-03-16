| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install --no-compile -q -r requirements-benchmark.txt` | 6.550 ± 0.388 | 6.210 | 6.973 | 4.56 ± 0.29 |
| `poetry install --compile -q --no-root --only main -E tqdm_feature` | 10.204 ± 0.262 | 10.013 | 10.502 | 7.11 ± 0.23 |
| `poetry install -q --no-root --only main -E tqdm_feature` | 4.399 ± 0.281 | 4.185 | 4.717 | 3.07 ± 0.21 |
| `/home/konsti/monotrail/target/release/monotrail poetry-install -E tqdm_feature` | 5.288 ± 0.086 | 5.223 | 5.386 | 3.69 ± 0.10 |
| `/home/konsti/monotrail/target/release/monotrail poetry-install --no-compile -E tqdm_feature` | 1.435 ± 0.029 | 1.402 | 1.453 | 1.00 |
