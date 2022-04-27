import os
import string
import sys
from pathlib import Path
from typing import List

from .get_pep508_env import get_pep508_env
from .monorail import prepare_monorail
from .monorail_path_finder import MonorailPathFinder


def load_monorail(filename: str, extras: List[str]):
    # Install all required packages and get their location (in rust)
    sprawl_root, sprawl_packages = prepare_monorail(
        filename, extras, get_pep508_env()
    )

    # Remove existing monorail path finder
    i = 0
    while i < len(sys.meta_path):
        if isinstance(sys.meta_path[i], MonorailPathFinder):
            sys.meta_path.remove(i)
        else:
            i = i + 1

    # activate new monorail path finder, making the packages loadable
    sys.meta_path.append(MonorailPathFinder(sprawl_root, sprawl_packages))


def monorail_from_env():
    """Small wrapper that calls into rust and instantiates the path finder"""
    # manually set through poetry-run
    # TODO: Should this be exposed and document or hidden away to obscurity
    filename = os.environ.get("MONORAIL_CWD")

    if not filename:
        # We're running before the debugger, so have to be hacky
        if Path(sys.argv[0]).name == "pydevd.py":
            filename = sys.argv[sys.argv.index("--file") + 1]
        else:
            # If we start python with no args, sys.argv is ['']
            filename = sys.argv[0]

    # remove the empty string
    if filename in [None, "-m", "-c"]:
        filename = None

    if extras := os.environ.get("MONORAIL_EXTRAS"):
        extras = extras.split(",")
    else:
        extras = []

    for extra in extras:
        # TODO extras normalization PEP
        # TODO non-ascii identifiers?
        if not set(extra) < set("_-" + string.ascii_letters + string.digits):
            raise ValueError("Invalid extra name '{}' allowed are underscore, minus, letters and digits")
    try:
        load_monorail(filename, extras)
    except Exception as e:
        print("VIRTUAL SPRAWL ERROR: PACKAGES WILL NOT BE AVAILABLE", e)
    except BaseException as e:  # Rust panic
        print("VIRTUAL SPRAWL CRASH (RUST PANIC): PACKAGES WILL NOT BE AVAILABLE", e)
