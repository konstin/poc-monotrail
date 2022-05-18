import os
import sys

from .monotrail import monotrail_from_args, project_name
from .monotrail_finder import MonotrailFinder


def load_monotrail():
    """Small wrapper that calls into rust and instantiates the finder"""
    # for some reason the last argument is missing with -m pytest (becomes -m),
    # and I have no idea how to debug where it went. esp since we sometimes cut the first arguments, but never the last
    # https://stackoverflow.com/a/62087608/3549270
    if sys.platform == "linux":
        args = (
            open("/proc/{}/cmdline".format(os.getpid())).read()[:-1].split("\x00")[1:]
        )
    else:
        args = sys.argv
    try:
        # Install all required packages and get their location (in rust)
        finder_data = monotrail_from_args(args)
    except Exception as e:
        print(
            f"{project_name.upper()} ERROR: PACKAGES WILL NOT BE AVAILABLE: {e}",
            file=sys.stderr,
        )
        return
    except BaseException as e:  # Rust panic
        print(
            f"{project_name.upper()} CRASH (RUST PANIC): PACKAGES WILL NOT BE AVAILABLE: {e}",
            file=sys.stderr,
        )
        return

    MonotrailFinder.get_singleton().update_and_activate(finder_data)
