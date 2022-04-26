"""
Loading this module will run virtual sprawl, installing all required packages and making them loadable
"""

import os

if os.environ.get("VIRTUAL_SPRAWL"):
    from .load_virtual_sprawl import virtual_sprawl_from_env

    virtual_sprawl_from_env()
