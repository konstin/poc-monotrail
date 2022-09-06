| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install -q -r requirements-benchmark.txt` | 11.745 ± 1.159 | 10.830 | 13.048 | 2.24 ± 0.23 |
| `poetry install -q --no-root -E tqdm_feature` | 15.039 ± 0.153 | 14.894 | 15.199 | 2.87 ± 0.08 |
| `monotrail poetry-install -E tqdm_feature` | 5.232 ± 0.135 | 5.089 | 5.357 | 1.00 |
