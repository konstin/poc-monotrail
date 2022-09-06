| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install -q -r requirements-benchmark.txt` | 29.481 ± 0.186 | 29.331 | 29.690 | 2.75 ± 0.37 |
| `poetry install -q --no-root -E import-json` | 70.291 ± 1.366 | 69.020 | 71.735 | 6.56 ± 0.88 |
| `monotrail poetry-install -E import-json` | 10.714 ± 1.423 | 9.504 | 12.282 | 1.00 |
