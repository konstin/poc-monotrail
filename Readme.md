# Proof Of Concept: Monotrail

This proof of concept shows how to get python packages without virtualenvs. It will install both python itself and your dependencies, given a `requirement.txt` or a `pyproject.toml` in the directory: 

```
monotrail run python my_script.py
```

Every dependency is installed only once globally and hooked to your code. No venv directory, no explicit installation, no activate, no pyenv.

This is a proof of concept, so only **most features are missing** and will crash or produce nonsense. E.g. only linux and macos are supported, installation is awkward, error messages are suboptimal, only pypi is supported, startup is really slow, only the most basic requirement.txt syntax is supported, lockfiles from interactive mode aren't saved, etc. 

monotrail means to show you can use python without the traditional "installing packages in an environment". It also integrates [PyOxy](https://github.com/indygreg/PyOxidizer/tree/main/pyoxy) so you don't need to install python anymore. 

There's also an interactive mode meant for notebooks, where you can add packages at runtime, get an isolated package set per notebook and avoid version conflicts.

```jupyterpython
!pip install monotrail
```

```jupyterpython
import monotrail

monotrail.interactive(
    numpy="^1.21",
    pandas="^1"
)
```

## Usage

With a python script, first download the binary and put it in PATH (e.g. via `.local/bin`). Make sure you have either a `requirements.txt` or `pyproject.toml`/`poetry.lock`

```
monotrail run python my_script.py
```

You can also run the scripts that used to be in `.venv/bin`:

```
monotrail run script pytest
```

There's also a python package with an entrypoint:

```
pip install monotrail
monotrail_python path/to/your/script.py
```

With jupyter notebooks

```jupyterpython
!pip install monotrail
```

```jupyterpython
import monotrail

monotrail.interactive(
    numpy="^1.21",
    pandas="^1"
)
```

Setting `RUST_LOG=debug` will give you details to track down bugs.

## Background

monotrail first parses which python version you want (3.8 by default) and if not present downloads it from [PyOxy](https://github.com/indygreg/PyOxidizer/tree/main/pyoxy). It doesn't run python as an executable but instead loads `libpython.so` and uses the [C API](https://docs.python.org/3/c-api/veryhigh.html).

Next, we search for a dependencies listing (`poetry.lock` or `requirements.txt`). If required we run poetry to resolve the dependencies (which we bootstrap through a pre-recorded `poetry.lock` for poetry itself). We install all missing packages to separate directories in `.cache/monotrail` and record all locations.

We initialize python and inject a custom [PathFinder](https://docs.python.org/3/library/importlib.html#importlib.machinery.PathFinder) with everything and add it to `sys.meta_path`. When python searches where `import` something from, it goes through all the `Finder`s in `sys.meta_path` until one returns a location. Ours knows the locations of the packages from the lockfile and python doesn't see anything else, so you can only load from the packages matching the lockfile. 

Interactive mode does pretty much the same, except we skip the python installation and there's a check that the version of an already imported package didn't change.

## Benchmarks (wheel installation)

One neat thing about venv-less installation is that we install every package version only once, so no more 3 different installations of pytorch. This takes a lot less disk space (even though clearing the cache is an unsolved problem) but most importantly it means that if you have used all required package versions once before "installation" is instantaneous. It also removes the need to recreate broken venvs.

By reimplementing wheel installation in rust, it also became a good bit faster. `install-wheel-rs` has a separate python interface so you can reuse it as a fast wheel installer on its own.

Installing a single large wheel (plotly)

```        
$ VIRTUAL_ENV=.venv-benchmark hyperfine -p ".venv-benchmark/bin/pip uninstall -y plotly" \
  "target/release/monotrail install wheels/plotly-5.5.0-py2.py3-none-any.whl" \
  ".venv-benchmark/bin/pip install --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl"
          
Benchmark #1: target/release/monotrail install wheels/plotly-5.5.0-py2.py3-none-any.whl
  Time (mean ± σ):      5.797 s ±  0.069 s    [User: 3.796 s, System: 1.979 s]
  Range (min … max):    5.699 s …  5.906 s    10 runs
 
Benchmark #2: .venv-benchmark/bin/pip install --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl
  Time (mean ± σ):      7.658 s ±  0.061 s    [User: 5.448 s, System: 2.085 s]
  Range (min … max):    7.598 s …  7.758 s    10 runs
 
Summary
  'target/release/monotrail install wheels/plotly-5.5.0-py2.py3-none-any.whl' ran
    1.32 ± 0.02 times faster than '.venv-benchmark/bin/pip install --no-deps wheels/plotly-5.5.0-py2.py3-none-any.whl'
```

A sample datascience stack (numpy, pandas, matplotlib)

```
test-data/poetry/data-science -E tqdm_feature
pip 22.0.4 from /home/konsti/monotrail/.venv-b/lib/python3.8/site-packages/pip (python 3.8)
Poetry version 1.1.13
Benchmark 1: .venv/bin/pip install -q -r requirements-benchmark.txt
  Time (mean ± σ):     11.745 s ±  1.159 s    [User: 9.637 s, System: 1.339 s]
  Range (min … max):   10.830 s … 13.048 s    3 runs
 
Benchmark 2: poetry install -q --no-root -E tqdm_feature
  Time (mean ± σ):     15.039 s ±  0.153 s    [User: 41.032 s, System: 5.934 s]
  Range (min … max):   14.894 s … 15.199 s    3 runs
 
Benchmark 3: monotrail poetry-install -E tqdm_feature
  Time (mean ± σ):      5.232 s ±  0.135 s    [User: 12.850 s, System: 2.334 s]
  Range (min … max):    5.089 s …  5.357 s    3 runs
 
Summary
  'monotrail poetry-install -E tqdm_feature' ran
    2.24 ± 0.23 times faster than '.venv/bin/pip install -q -r requirements-benchmark.txt'
    2.87 ± 0.08 times faster than 'poetry install -q --no-root -E tqdm_feature'
```

A mid-sized django project 

```
test-data/poetry/mst -E import-json
pip 22.0.4 from /home/konsti/monotrail/.venv-b/lib/python3.8/site-packages/pip (python 3.8)
Poetry version 1.1.13
Benchmark 1: .venv/bin/pip install -q -r requirements-benchmark.txt
  Time (mean ± σ):     29.481 s ±  0.186 s    [User: 21.001 s, System: 3.313 s]
  Range (min … max):   29.331 s … 29.690 s    3 runs
 
Benchmark 2: poetry install -q --no-root -E import-json
  Time (mean ± σ):     70.291 s ±  1.366 s    [User: 355.966 s, System: 46.958 s]
  Range (min … max):   69.020 s … 71.735 s    3 runs
 
Benchmark 3: monotrail poetry-install -E import-json
  Time (mean ± σ):     10.714 s ±  1.423 s    [User: 43.583 s, System: 10.551 s]
  Range (min … max):    9.504 s … 12.282 s    3 runs
 
Summary
  'monotrail poetry-install -E import-json' ran
    2.75 ± 0.37 times faster than '.venv/bin/pip install -q -r requirements-benchmark.txt'
    6.56 ± 0.88 times faster than 'poetry install -q --no-root -E import-json'
```

# Dev setup

I use two venvs, the one I have activated is called `.venv-b` and another `.venv` where I install monotrail. Setup is

```bash
virtualenv .venv-b
. .venv-b/bin/activate
pip install -U maturin[zig] black pip pytest ipython
virtualenv .venv
# pytest runs the tests, jupyter nbconvert is for testing the notebook
.venv/bin/pip install pytest jupyter nbconvert
```