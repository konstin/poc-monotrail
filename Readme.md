# Proof Of Concept: Virtual Sprawl

This proof of concept shows two things:

 a) how to install packages faster than pip/poetry - see benchmarks below
 b) venv-less python packages: every dependency is installed only once and hooked to your project. No more venv directory.


```        
$ VIRTUAL_ENV=.venv-benchmark hyperfine -p ".venv-benchmark/bin/pip uninstall -y plotly" \
  "target/release/virtual-sprawl install wheels/plotly-5.5.0-py2.py3-none-any.whl" \
  ".venv-benchmark/bin/pip install --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl"
          
Benchmark #1: target/release/virtual-sprawl install wheels/plotly-5.5.0-py2.py3-none-any.whl
  Time (mean ± σ):      5.797 s ±  0.069 s    [User: 3.796 s, System: 1.979 s]
  Range (min … max):    5.699 s …  5.906 s    10 runs
 
Benchmark #2: .venv-benchmark/bin/pip install --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl
  Time (mean ± σ):      7.658 s ±  0.061 s    [User: 5.448 s, System: 2.085 s]
  Range (min … max):    7.598 s …  7.758 s    10 runs
 
Summary
  'target/release/virtual-sprawl install wheels/plotly-5.5.0-py2.py3-none-any.whl' ran
    1.32 ± 0.02 times faster than '.venv-benchmark/bin/pip install --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl'
```