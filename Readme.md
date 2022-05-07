# Proof Of Concept: Monotrail

This proof of concept shows how to do python package management without virtualenvs.

Every dependency is installed only once globally and hooked to your project from your lockfile. No more venv directory.

This is a proof of concept, so only **most features are missing** and will crash or produce nonsense. E.g. only linux and macos are supported, installation is awkward, error messages are suboptimal, only pypi is supported, only the most basic requirement.txt syntax is supported, lockfiles from interactive mode aren't saved, etc. 

monotrail means to show you can effectively just clone a repo with a lockfile and run a single command that install all required packages, makes them available to `import` and then runs your script, skipping explicit package management, `.venv` directories and installing the same dependency for each project again.

```
MONOTRAIL=1 python path/to/your/script.py
```

In the background, monotrail uses a `.pth` hook which runs on python startup before your code to set everything up.

It also features an interactive mode, where you can add packages at runtime, get an isolated package set per notebook and don't get any version conflicts.

```jupyterpython
import monotrail

monotrail.interactive(
    numpy="^1.21",
    pandas="^1"
)
```

## Usage

With a python script:

```
pip install -U pip virtualenv maturin
virtualenv .venv
#.venv/bin/pip install monotrail
MONOTRAIL=1 .venv/bin/python path/to/your/script.py
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

_wait, you said venv-less!_ We need to install a `.pth` hook and I don't want to pollute your user-global environment, so we isolate it in a venv you can just `rm -rf`. You can use the resulting .venv for all of your projects while still having isolation (it would of course be a lot cooler to have `monotrail +3.8 run path/to/your/script.py` but I don't know how to dynamically load, import-hook and launch a user-specified python version. If you do, please tell me!)

To run scripts, use `/path/to/.venv/bin/python -m monotrail.run <your script> <args>`.

To install extras, use `MONOTRAIL_EXTRAS="extra1,extra2"`. With `MONOTRAIL_ROOT` you can change the storage location if you really need to. Setting `RUST_LOG=debug` will give you many more details.

## Background

Python has a module called [site](https://docs.python.org/3/library/site.html) which runs automatically and if you're using a virtualenv environment, it makes the packages in there available. You can also use it to run arbitrary code by placing a `.pth` file in `site-packages`, so we install a `load_monotrail.pth`. This import our `monotrail` module, and `if os.environ.get("MONOTRAIL")`, it loads the actual machinery. It's a small shim that calls into rust. There we search for a dependencies listing (`poetry.lock` or `requirements.txt`), install all packages if not installed and return their location of all installed dependencies. Back in python, we build a custom [PathFinder](https://docs.python.org/3/library/importlib.html#importlib.machinery.PathFinder) with everything and add it to `sys.meta_path`. When python searches where `import` something from, it goes through all the `Finder`s in `sys.meta_path` until one returns a location. Ours returns the location of the locked packages in the global package installation location in your cache dir (`.cache` on linux).

Interactive mode is very similar, except we only create the `PathFinder` on the first call to `monotrail.interactive()`.

## Benchmarks

One neat thing about venv-less installation is that we install every package version only once, so no more 3 different installations of pytorch. This takes a lot less disk space (even though clearing the cache is an unsolved problem) but most importantly it means that if you have used all required package versions once before "installation" is instantaneous. It also removes the need to recreate broken venvs.

By reimplementing wheel installation in rust, it also became a good bit faster. 

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