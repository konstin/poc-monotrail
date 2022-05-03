"""
Loading this module will run monotrail, installing all required packages and making them loadable
"""

import os
from typing import Optional

if os.environ.get("MONOTRAIL"):
    from .load_monotrail import load_monotrail

    load_monotrail()

_lockfile: Optional[str] = None


def interactive(**kwargs):
    """For use in e.g. jupyter notebook: Use the first cell to define what you need and add stuff as you go."""
    global _lockfile
    # enums with fields (or even untagged enums) are unsupported by pyo3, so json it is
    from .monotrail import monotrail_from_requested
    from .monotrail_path_finder import MonotrailPathFinder
    import json

    sprawl_root, sprawl_packages, _lockfile = monotrail_from_requested(json.dumps(kwargs), _lockfile)
    MonotrailPathFinder.get_singleton().update_and_activate(
        sprawl_root, sprawl_packages
    )
