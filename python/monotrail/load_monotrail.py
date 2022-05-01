import os
import sys

from .monotrail import prepare_monotrail_from_env
from .monotrail_path_finder import MonotrailPathFinder


def load_monotrail():
    """Small wrapper that calls into rust and instantiates the path finder"""
    # https://stackoverflow.com/a/62087608/3549270
    # for some reason the last argument is missing with -m pytest (becomes -m),
    # and i have no idea how to debug where it went. esp since we sometimes cut the first arguments, but never the last
    if sys.platform == "linux":
        args = open("/proc/{}/cmdline".format(os.getpid())).read()[:-1].split("\x00")[1:]
    else:
        args = sys.argv
    try:
        # Install all required packages and get their location (in rust)
        sprawl_root, sprawl_packages = prepare_monotrail_from_env(args)
    except Exception as e:
        print("MONOTRAIL ERROR: PACKAGES WILL NOT BE AVAILABLE", e)
        return
    except BaseException as e:  # Rust panic
        print("MONOTRAIL CRASH (RUST PANIC): PACKAGES WILL NOT BE AVAILABLE", e)
        return

    # Remove existing monotrail path finder (if any)
    i = 0
    while i < len(sys.meta_path):
        if isinstance(sys.meta_path[i], MonotrailPathFinder):
            sys.meta_path.remove(i)
        else:
            i = i + 1

    # activate new monotrail path finder, making the packages loadable
    sys.meta_path.append(MonotrailPathFinder(sprawl_root, sprawl_packages))
