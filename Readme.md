# Proof Of Concept: Virtual Sprawl

This proof of concept shows two things:

a) how to install packages faster than pip/poetry - see benchmarks below

b) venv-less python packages: every dependency is installed only once globally and hooked to your project from your lockfile. No more venv directory.

While a) is a nice gimmick (and could be integrated into pip/poetry), b) is where the real magic happens, so the reminder of the readme is going to focus on this. This is a proof of concept, so only **many features are missing** and will crash or produce nonsense. E.g. only linux and macos are supported, you need a lockfile created by another tool, installation is awkward, error messages are suboptimal, non-pypi version tracking is broken, etc. 

virtual sprawl means to show you can effectively just clone a repo with a lockfile and run a single command that install all required packages, makes them available to `import` and then runs your script, skipping explicit package management, `.venv` directories and installing the same dependency for each project again.

```
VIRTUAL_SPRAWL=1 python path/to/your/script.py
```

In the background, virtual sprawl uses a `.pth` hook which runs on python startup before your code to set everything up and 

## Usage

```
pip install virtualenv
virtualenv .venv
.venv/bin/pip install virtual-sprawl
.venv/bin/python path/to/your/script.py
```

_wait, you said venv-less!_ We need to install a `.pth` hook and I don't want to pollute your user-global environment, so we isolate it in a venv you can just `rm -rf`. (it would of course be a lot cooler to have `virtual_sprawl +3.8 run path/to/your/script.py` but I don't know how to dynamically load, import-hook and launch a user-specified python version. If you do, please tell me!)

## Debugging

Setting `RUST_LOG=debug` will give you many more details.

## Background

Python has a module called [site](https://docs.python.org/3/library/site.html) which runs automatically and if you're using a virtualenv environment, it makes the packages in there available. You can also use it to run arbitrary code by placing a `.pth` file in `site-packages`, so we install a `load_virtual_sprawl.pth`. This import our `virtual_sprawl` module, and `if os.environ.get("VIRTUAL_SPRAWL")`, it loads our actual machinery. First, we call into rust. There we search for a lockfile (`poetry.lock` or `requirements-frozen.txt`), install all packages still missing and return all the location of all installed dependencies. Back in python, we build a custom [PathFinder](https://docs.python.org/3/library/importlib.html#importlib.machinery.PathFinder) with all packages and add it to `sys.meta_path`. When python searches where `import` something from, it goes through all the `PathFinder` until one returns a location. Ours returns the location in of the locked packages in the global package installation location in your cache dir (`.cache` on linux).

## Benchmarks

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