"""
Loading this module will run monotrail, installing all required packages and making
them loadable
"""
import os
import sys
from typing import Optional, List
from ._monotrail_finder import MonotrailFinder
from .monotrail import (
    monotrail_from_args,
    monotrail_from_requested,
    monotrail_from_git,
    monotrail_from_dir,
    monotrail_spec_paths,
    monotrail_find_scripts,
)

__all__ = [
    "interactive",
    "from_git",
    "monotrail_from_args",
    "monotrail_from_requested",
    "monotrail_from_git",
    "monotrail_from_dir",
    "monotrail_spec_paths",
    "monotrail_find_scripts",
    "MonotrailFinder",
]

# checks for MONOTRAIL=1, but we can't use project_name here because we don't want to
# load the rust part if it's not requested
if os.environ.get(sys.modules[__name__].__name__.upper()):
    from ._load_monotrail import load_monotrail

    load_monotrail()


def interactive(**kwargs):
    """For use in e.g. jupyter notebook: Use the first cell to define what you need and
    add stuff as you go.

    ```python
    import monotrail

    monotrail.interactive(
        numpy="^1.21",
        pandas="^1"
    )
    ```
    """
    from .monotrail import monotrail_from_requested
    from ._monotrail_finder import MonotrailFinder
    import json

    # enums with fields (or even untagged enums) are unsupported by pyo3, so json it is
    finder = MonotrailFinder.get_singleton()
    inject_data = monotrail_from_requested(json.dumps(kwargs), finder.lockfile)
    finder.update_and_activate(inject_data)


def from_git(repo_url: str, revision: str, extras: Optional[List[str]] = None):
    """For deploying repositories in e.g. jupyter notebook: Use the first cell with the
    repository and a tag and you'll have all deps and repo code available

    ```python
    import monotrail

    monotrail.from_git(
        "https://github.com/sokrypton/ColabFold",
        "63b42b8f5b5da418efecf6c4d11490a96595020d"
    )
    ```
    """
    from .monotrail import monotrail_from_git
    from ._monotrail_finder import MonotrailFinder

    finder = MonotrailFinder.get_singleton()
    inject_data = monotrail_from_git(repo_url, revision, extras, finder.lockfile)
    finder.update_and_activate(inject_data)
