| Command | Mean [s] | Min [s] | Max [s] | Relative |
|:---|---:|---:|---:|---:|
| `.venv/bin/pip install -q -r requirements-benchmark.txt` | 54.001 ± 31.527 | 34.221 | 90.359 | 4.65 ± 2.75 |
| `poetry install -q --no-root $BENCHMARK_OPTIONS` | 83.132 ± 5.611 | 79.385 | 89.583 | 7.16 ± 0.83 |
| `../../../target/x86_64-unknown-linux-musl/release/monorail poetry-install $BENCHMARK_OPTIONS` | 11.614 ± 1.102 | 10.597 | 12.785 | 1.00 |
