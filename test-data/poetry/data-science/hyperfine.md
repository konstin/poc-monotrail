| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install -q -r requirements-benchmark.txt` | 11.877 ± 0.486 | 11.440 | 12.401 | 2.17 ± 0.10 |
| `poetry install -q --no-root -E tqdm_feature` | 15.106 ± 0.322 | 14.911 | 15.477 | 2.77 ± 0.09 |
| `monotrail poetry-install -E tqdm_feature` | 5.463 ± 0.138 | 5.329 | 5.604 | 1.00 |
