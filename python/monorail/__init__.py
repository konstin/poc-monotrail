"""
Loading this module will run monorail, installing all required packages and making them loadable
"""

import os

if os.environ.get("MONORAIL"):
    from .load_monorail import load_monorail

    load_monorail()
