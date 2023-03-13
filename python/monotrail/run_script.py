"""
Like `monotrail ru script`, but as a python entrypoint

The major missing point is setting up the execve environment
"""

import importlib
import os
import runpy
import sys

from .monotrail import monotrail_from_args, project_name, monotrail_find_scripts
from ._monotrail_finder import MonotrailFinder


def main():
    # first arg is always the current script
    if len(sys.argv) == 1:
        print("Missing executable name", file=sys.stderr)
        sys.exit(1)

    script_name = sys.argv[1]
    # Install all required packages and get their location (in rust)
    finder_data = monotrail_from_args([])

    # Search poetry scripts
    if script_name in finder_data.root_scripts:
        # Otherwise, imports from the current projects won't work
        if f"{project_name.upper()}_CWD" in os.environ:
            sys.path.append(os.environ[f"{project_name.upper()}_CWD"])
        else:
            sys.path.append(os.getcwd())
        # prepare execution environment
        MonotrailFinder.get_singleton().update_and_activate(finder_data)
        script = finder_data.root_scripts[script_name]
        # See https://packaging.python.org/en/latest/specifications/entry-points/#data-model
        obj = importlib.import_module(script.module)
        if script.function:
            for attr in script.function.split("."):
                obj = getattr(obj, attr)
        # noinspection PyCallingNonCallable
        # it's required to be a callable module if no function name is provided,
        # otherwise we error
        sys.exit(obj())

    scripts = monotrail_find_scripts(
        finder_data.sprawl_root, finder_data.sprawl_packages
    )
    script_path = scripts.get(script_name)
    if not script_path:
        print(f"Couldn't find '{script_name}' in installed packages", file=sys.stderr)
        sys.exit(1)

    # https://sphinx-locales.github.io/peps/pep-0427/#recommended-installer-features
    # > In wheel, scripts are packaged in {distribution}-{version}.data/scripts/.
    # > If the first line of a file in scripts/ starts with exactly b'#!python',
    # > rewrite to point to the correct interpreter. Unix installers may need to
    # > add the +x bit to these files if the archive was created on Windows.
    #
    # > The b'#!pythonw' convention is allowed. b'#!pythonw' indicates a GUI script
    # > instead of a console script.
    #
    # We do this in venvs as required, but in monotrail mode we use a fake shebang
    # (#!/usr/bin/env python) for injection monotrail as python into PATH later
    placeholder_python = b"#!/usr/bin/env python"
    with open(script_path, "rb") as file:
        shebang = file.read(len(placeholder_python))

    # sys.argv[0] must be the full path to the current script
    sys.argv = [script_path] + sys.argv[2:]
    if shebang == placeholder_python:
        # Case 1: it's a python script
        # prepare execution environment
        MonotrailFinder.get_singleton().update_and_activate(finder_data)
        runpy.run_path(str(script_path), run_name="__main__")
    else:
        # Case 2: it's not a python script, e.g. a native executable or a bash script
        # replace current process or it feels more native
        os.execv(script_path, sys.argv)


if __name__ == "__main__":
    main()
