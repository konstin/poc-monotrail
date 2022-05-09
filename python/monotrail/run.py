import importlib
import os
import sys
from pathlib import Path

from .monotrail import monotrail_from_env
from .monotrail_path_finder import MonotrailPathFinder


def main():
    # arg 1 is always the current script
    if len(sys.argv) == 1:
        print("Missing executable name", file=sys.stderr)
        sys.exit(1)

    script_name = sys.argv[1]
    # If you triggered the pth autoload beforehand, it'll have already done the wrong thing by this step
    if os.environ.get("MONOTRAIL"):
        print(
            "warning: you're using monotrail.run with MONOTRAIL=1 being set, this will cause problems",
            file=sys.stderr,
        )

    # Install all required packages and get their location (in rust)
    sprawl_root, sprawl_packages, scripts = monotrail_from_env([])

    if script_name in scripts:
        # Otherwise imports from the current projects won't work
        if "MONOTRAIL_CWD" in os.environ:
            sys.path.append(os.environ["MONOTRAIL_CWD"])
        else:
            sys.path.append(os.getcwd())
        # prepare execution environment
        MonotrailPathFinder.get_singleton().update_and_activate(
            sprawl_root, sprawl_packages
        )
        object_ref = scripts[script_name]
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
    for package in sprawl_packages:
        script_path = (
            Path(package.monotrail_location(sprawl_root))
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
        MonotrailPathFinder.get_singleton().update_and_activate(
            sprawl_root, sprawl_packages
        )
        with open(script_path) as file:
            # We use compile to attach the filename for debuggability
            python_script = compile(file.read(), script_path, "exec")
        # Exec keeps the `__name__ == "__main__"` part and keeps the cli args
        exec(python_script)
    else:
        # Case 2: it's not a python script, e.g. a native executable or a bash script
        # replace current process or it feels more native
        os.execv(script_path, sys.argv)


if __name__ == "__main__":
    main()
