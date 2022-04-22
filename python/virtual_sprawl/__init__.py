import os

if os.environ.get("VIRTUAL_SPRAWL"):
    from .load_virtual_sprawl import load_virtual_sprawl

    load_virtual_sprawl()
