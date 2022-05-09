import os
import sys

from .monotrail import monotrail_from_env
from .monotrail_path_finder import MonotrailPathFinder


def load_monotrail():
    """Small wrapper that calls into rust and instantiates the path finder"""
    # for some reason the last argument is missing with -m pytest (becomes -m),
    # and i have no idea how to debug where it went. esp since we sometimes cut the first arguments, but never the last
    # https://stackoverflow.com/a/62087608/3549270
    if sys.platform == "linux":
        args = (
            open("/proc/{}/cmdline".format(os.getpid())).read()[:-1].split("\x00")[1:]
        )
    else:
        args = sys.argv
    try:
        # Install all required packages and get their location (in rust)
        sprawl_root, sprawl_packages, script = monotrail_from_env(args)
    except Exception as e:
        print("MONOTRAIL ERROR: PACKAGES WILL NOT BE AVAILABLE", e)
        return
    except BaseException as e:  # Rust panic
        print("MONOTRAIL CRASH (RUST PANIC): PACKAGES WILL NOT BE AVAILABLE", e)
        return

    MonotrailPathFinder.get_singleton().update_and_activate(
        sprawl_root, sprawl_packages
    )
