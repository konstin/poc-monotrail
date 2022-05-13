"""
Loading this module will run monotrail, installing all required packages and making them loadable
"""
import os
import sys
from typing import Optional, List

# checks for MONOTRAIL=1, but we can't use project_name here because we don't want to load the rust part if
# it's not requested
if os.environ.get(sys.modules[__name__].__name__.upper()):
    from .load_monotrail import load_monotrail

    load_monotrail()


def interactive(**kwargs):
    """For use in e.g. jupyter notebook: Use the first cell to define what you need and add stuff as you go."""
    from .monotrail import monotrail_from_requested
    from .monotrail_finder import MonotrailFinder
    import json

    # enums with fields (or even untagged enums) are unsupported by pyo3, so json it is
    finder = MonotrailFinder.get_singleton()
    finder_data = monotrail_from_requested(json.dumps(kwargs), finder.lockfile)
    finder.update_and_activate(finder_data)


def from_git(repo_url: str, revision: str, extras: Optional[List[str]] = None):
    """For deploying repositories in e.g. jupyter notebook: Use the first cell with the repository and a tag and you'll
    have all deps and repo code available"""
    from .monotrail import monotrail_from_git
    from .monotrail_finder import MonotrailFinder

    finder = MonotrailFinder.get_singleton()
    finder_data = monotrail_from_git(repo_url, revision, extras, finder.lockfile)
    finder.update_and_activate(finder_data)
