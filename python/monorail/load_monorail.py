import os
import sys

from .monorail import prepare_monorail_from_env
from .monorail_path_finder import MonorailPathFinder


def load_monorail():
    """Small wrapper that calls into rust and instantiates the path finder"""
    # https://stackoverflow.com/a/62087608/3549270
    # for some reason the last argument is missing with -m pytest (becomes -m),
    # and i have no idea how to debug where it went. esp since we sometimes cut the first arguments, but never the last
    args = open("/proc/{}/cmdline".format(os.getpid())).read()[:-1].split("\x00")[1:]
    try:
        # Install all required packages and get their location (in rust)
        sprawl_root, sprawl_packages = prepare_monorail_from_env(args)
    except Exception as e:
        print("MONORAIL ERROR: PACKAGES WILL NOT BE AVAILABLE", e)
        return
    except BaseException as e:  # Rust panic
        print("MONORAIL CRASH (RUST PANIC): PACKAGES WILL NOT BE AVAILABLE", e)
        return

    # Remove existing monorail path finder (if any)
    i = 0
    while i < len(sys.meta_path):
        if isinstance(sys.meta_path[i], MonorailPathFinder):
            sys.meta_path.remove(i)
        else:
            i = i + 1

    # activate new monorail path finder, making the packages loadable
    sys.meta_path.append(MonorailPathFinder(sprawl_root, sprawl_packages))
