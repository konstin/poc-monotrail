#!/usr/bin/env python3
import glob
import os
import platform
from pathlib import Path
from subprocess import check_call

for wheel in glob.glob("target-maturin/wheels/monotrail-*.whl"):
    os.remove(wheel)

check_call(
    ["maturin", "build", "--release", "--strip"],
    env=dict(os.environ, CARGO_TARGET_DIR="target-maturin"),
)
check_call(["virtualenv", "-p", "3.8", ".venv"])

if platform.system() == "Windows":
    bin_dir = Path(".venv").joinpath("Scripts")
    pip = bin_dir.joinpath("pip.exe")
else:
    bin_dir = Path(".venv").joinpath("bin")
    pip = bin_dir.joinpath("pip")

[monotrail_wheel] = glob.glob("target-maturin/wheels/monotrail-*.whl")
check_call([pip, "install", "--force-reinstall", monotrail_wheel])
print("Installed")


check_call(
    [bin_dir.joinpath("monotrail_python"), "data_science_project/import_pandas.py"]
)
check_call([bin_dir.joinpath("monotrail_python"), "data_science_project/make_paper.py"])
check_call(
    [bin_dir.joinpath("monotrail_script"), "numpy_identity_3"],
    env=dict(os.environ, MONOTRAIL_CWD="data_science_project"),
)

# .venv/bin/monotrail_python flipstring/flip.py "hello world!"
# .venv/bin/pytest test/python

if __name__ == "__main__":
    pass
