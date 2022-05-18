import runpy
import sys
from pathlib import Path

from .monotrail import monotrail_from_args
from .monotrail_finder import MonotrailFinder


def main():
    # ugly reimplementation of python's cli args
    # first arg is always the current script
    if sys.argv[1] == "-m":
        module = sys.argv[2]
        finder_data = monotrail_from_args([])
        MonotrailFinder.get_singleton().update_and_activate(finder_data)
        sys.argv = sys.argv[2:]  # cut away `run_python` and `-m`
        runpy.run_module(module, run_name="__main__")
    elif sys.argv[1] == "-c":
        code = sys.argv[2]
        finder_data = monotrail_from_args([])
        MonotrailFinder.get_singleton().update_and_activate(finder_data)
        exec(code)
    else:
        script = sys.argv[1]
        if not Path(script).is_file():
            print(f"No such file: {script}")
        sys.path.insert(0, str(Path(script).parent))
        finder_data = monotrail_from_args([script])
        MonotrailFinder.get_singleton().update_and_activate(finder_data)
        sys.argv = sys.argv[1:]  # cut away `run_python`
        runpy.run_path(script, run_name="__main__")


if __name__ == "__main__":
    main()
