import os
import sys
from pathlib import Path

from .get_pep508_env import get_pep508_env
from .virtual_sprawl import prepare_virtual_sprawl

# arg 1 is always the current script
if len(sys.argv) == 1:
    print("Missing executable name", file=sys.stderr)
    sys.exit(1)

script_name = sys.argv[1]
if not os.environ.get("VIRTUAL_SPRAWL"):
    os.execv(script_name, sys.argv[1:])

if extras := os.environ.get("VIRTUAL_SPRAWL_EXTRAS"):
    extras = extras.split(",")
else:
    extras = []
# Install all required packages and get their location (in rust)
sprawl_root, sprawl_packages = prepare_virtual_sprawl(None, extras, get_pep508_env())

# Find the actual location of the entrypoint
for package in sprawl_packages:
    script_path = (
        Path(sprawl_root)
        .joinpath(package.name)
        .joinpath(package.unique_version)
        .joinpath(package.tag)
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
    with open(script_path) as file:
        python_script = file.read()
    # Exec keeps the `__name__ == "__main__"` part and keeps the cli args
    exec(python_script)
else:
    # Case 2: it's not a python script, e.g. a native executable or a bash script
    # replace current process or it feels more native
    print(script_path, sys.argv)
    os.execv(script_path, sys.argv)
