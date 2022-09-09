| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install -q -r requirements-benchmark.txt` | 11.061 ± 0.179 | 10.911 | 11.258 | 2.09 ± 0.08 |
| `poetry install -q --no-root --only main -E tqdm_feature` | 16.234 ± 2.434 | 14.037 | 18.851 | 3.07 ± 0.47 |
| `monotrail poetry-install -E tqdm_feature` | 5.283 ± 0.189 | 5.085 | 5.462 | 1.00 |
