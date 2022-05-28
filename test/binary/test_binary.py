import shutil
import subprocess

import pytest

from test.install.utils import get_bin


@pytest.mark.parametrize("version", ["3.8", "3.9"])
def test_datascience(version, pytestconfig):
    output = subprocess.check_output(
        [
            get_bin(),
            "run",
            "-p",
            version,
            "python",
            pytestconfig.rootpath.joinpath("data_science_project").joinpath(
                "import_pandas.py"
            ),
        ],
        text=True,
    )
    assert output.splitlines()[-1].strip() == "1.4.2"
    # .venv/bin/monotrail_python data_science_project/make_paper.py


def test_flipstring(pytestconfig):
    output = subprocess.check_output(
        [
            get_bin(),
            "run",
            "python",
            pytestconfig.rootpath.joinpath("flipstring").joinpath("flip.py"),
            "hello world!",
        ],
        text=True,
    )
    assert output.splitlines()[-1] == "¡pꞁɹoM oꞁꞁǝH"


@pytest.mark.parametrize("version", ["3.8", "3.9"])
@pytest.mark.skip(reason="TODO")
def test_jupyter(version, pytestconfig, tmp_path):
    jupyter_launcher = pytestconfig.rootpath.joinpath("jupyter-launcher")
    temp_notebook = tmp_path.joinpath("version.ipynb")
    shutil.copyfile(jupyter_launcher.joinpath("version.ipynb"), temp_notebook)
    output = subprocess.check_output(
        [
            get_bin(),
            "run",
            "-p",
            version,
            "script",
            "jupyter",
            "nbconvert",
            "--inplace",
            "--execute",
            temp_notebook,
        ],
        cwd=jupyter_launcher,
        text=True,
    )
    assert output.splitlines()[-1] == version


def test_tox(pytestconfig):
    """Run the same command across multiple python versions"""
    data_science_project = pytestconfig.rootpath.joinpath("data_science_project")
    command = [
        get_bin(),
        "run",
        "-p",
        "3.8",
        "-p",
        "3.9",
        "-p",
        "3.10",
        "python",
        "numpy_version.py",
    ]
    output = subprocess.check_output(
        command,
        cwd=data_science_project,
        text=True,
    )
    hellos = list(filter(lambda line: line.startswith("hi from"), output.splitlines()))
    assert hellos == [
        "hi from python 3.8 and numpy 1.22.3",
        "hi from python 3.9 and numpy 1.22.3",
        "hi from python 3.10 and numpy 1.22.3",
    ]
