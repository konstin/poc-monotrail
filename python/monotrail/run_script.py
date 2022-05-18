import importlib
import os
import runpy
import sys
from pathlib import Path

from .monotrail import monotrail_from_args, project_name
from .monotrail_finder import MonotrailFinder


def main():
    # first arg is always the current script
    if len(sys.argv) == 1:
        print("Missing executable name", file=sys.stderr)
        sys.exit(1)

    script_name = sys.argv[1]
    # Install all required packages and get their location (in rust)
    finder_data = monotrail_from_args([])

    if script_name in finder_data.scripts:
        # Otherwise, imports from the current projects won't work
        if f"{project_name.upper()}_CWD" in os.environ:
            sys.path.append(os.environ[f"{project_name.upper()}_CWD"])
        else:
            sys.path.append(os.getcwd())
        # prepare execution environment
        MonotrailFinder.get_singleton().update_and_activate(finder_data)
        object_ref = finder_data.scripts[script_name]
        # code from https://packaging.python.org/en/latest/specifications/entry-points/#data-model
        modname, qualname_separator, qualname = object_ref.partition(":")
        obj = importlib.import_module(modname)
        if qualname_separator:
            for attr in qualname.split("."):
                obj = getattr(obj, attr)
        # noinspection PyCallingNonCallable
        # it's required to be a callable module if no function name is provided, otherwise we error
        sys.exit(obj())

    # Find the actual location of the entrypoint
    for package in finder_data.sprawl_packages:
        script_path = (
            Path(package.monotrail_location(finder_data.sprawl_root))
            .joinpath("bin")
            .joinpath(script_name)
        )
        if script_path.is_file():
            break
    else:
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
    placeholder_python = b"#!python"
    with open(script_path, "rb") as file:
        shebang = file.read(len(placeholder_python))

    # sys.argv[0] must be the full path to the current script
    sys.argv = [str(script_path.absolute())] + sys.argv[2:]
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
