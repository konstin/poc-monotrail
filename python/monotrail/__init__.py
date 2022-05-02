"""
Loading this module will run monotrail, installing all required packages and making them loadable
"""

import os

if os.environ.get("MONOTRAIL"):
    from .load_monotrail import load_monotrail

    load_monotrail()


def interactive(**kwargs):
    # enums with fields (or even untagged enums) are unsupported by pyo3, so json it is
    from .monotrail import monotrail_from_requested
    from .monotrail_path_finder import MonotrailPathFinder
    import json

    sprawl_root, sprawl_packages = monotrail_from_requested(json.dumps(kwargs))
    MonotrailPathFinder.get_singleton().update_and_activate(
        sprawl_root, sprawl_packages
    )
