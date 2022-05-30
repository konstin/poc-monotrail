import platform

import nbformat
import pytest
from nbconvert.preprocessors import (
    ExecutePreprocessor,
    ClearOutputPreprocessor,
    ClearMetadataPreprocessor,
)


@pytest.mark.parametrize("notebook", ["interactive.ipynb", "from_git.ipynb"])
def test_interactive(notebook, pytestconfig):
    if notebook == "from_git.ipynb":
        pytest.skip("CI fails macos")
    notebook_dir = pytestconfig.rootpath.joinpath("test").joinpath("python")
    with notebook_dir.joinpath(notebook).open() as f:
        nb = nbformat.read(f, as_version=4)

    ep = ExecutePreprocessor(timeout=60)
    ep.preprocess(
        nb,
        {
            "metadata": {"path": str(notebook_dir)},
            "ClearOutputPreprocessor": {"enabled": True},
        },
    )

    # clear output; the execution by itself doesn't write output, but this also needs to be done after-edit pre-commit
    ClearOutputPreprocessor().preprocess(nb, {})
    ClearMetadataPreprocessor().preprocess(nb, {})
    with notebook_dir.joinpath(notebook).open("w", encoding="utf-8") as f:
        nbformat.write(nb, f)
