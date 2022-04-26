import os
import string
import sys
from pathlib import Path
from typing import List

from .get_pep508_env import get_pep508_env
from .virtual_sprawl import prepare_virtual_sprawl
from .virtual_sprawl_path_finder import VirtualSprawlPathFinder


def load_virtual_sprawl(filename: str, extras: List[str]):
    # Install all required packages and get their location (in rust)
    sprawl_root, sprawl_packages = prepare_virtual_sprawl(
        filename, extras, get_pep508_env()
    )

    # Remove existing virtual sprawl path finder
    i = 0
    while i < len(sys.meta_path):
        if isinstance(sys.meta_path[i], VirtualSprawlPathFinder):
            sys.meta_path.remove(i)
        else:
            i = i + 1

    # activate new virtual sprawl path finder, making the packages loadable
    sys.meta_path.append(VirtualSprawlPathFinder(sprawl_root, sprawl_packages))


def virtual_sprawl_from_env():
    """Small wrapper that calls into rust and instantiates the path finder"""
    # manually set through poetry-run
    # TODO: Should this be exposed and document or hidden away to obscurity
    filename = os.environ.get("VIRTUAL_SPRAWL_CWD")

    if not filename:
        # We're running before the debugger, so have to be hacky
        if Path(sys.argv[0]).name == "pydevd.py":
            filename = sys.argv[sys.argv.index("--file") + 1]
        else:
            # If we start python with no args, sys.argv is ['']
            filename = sys.argv[0]

    # remove the empty string
    if not filename or filename == "-m":
        filename = None

    if extras := os.environ.get("VIRTUAL_SPRAWL_EXTRAS"):
        extras = extras.split(",")
    else:
        extras = []

    for extra in extras:
        # TODO extras normalization PEP
        # TODO non-ascii identifiers?
        if not set(extra) < set("_-" + string.ascii_letters + string.digits):
            raise ValueError("Invalid extra name '{}' allowed are underscore, minus, letters and digits")
    try:
        load_virtual_sprawl(filename, extras)
    except Exception as e:
        print("VIRTUAL SPRAWL ERROR: PACKAGES WILL NOT BE AVAILABLE", e)
    except BaseException as e:  # Rust panic
        print("VIRTUAL SPRAWL CRASH (RUST PANIC): PACKAGES WILL NOT BE AVAILABLE", e)
