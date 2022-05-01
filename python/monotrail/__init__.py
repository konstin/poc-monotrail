"""
Loading this module will run monotrail, installing all required packages and making them loadable
"""

import os

if os.environ.get("MONOTRAIL"):
    from .load_monotrail import load_monotrail

    load_monotrail()
