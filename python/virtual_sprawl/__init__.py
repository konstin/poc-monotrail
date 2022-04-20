import os

if os.environ.get("VIRTUAL_SPRAWL"):
    from pathlib import Path
    import sys
    from .get_pep508_env import get_pep508_env
    from .virtual_sprawl_path_finder import VirtualSprawlPathFinder
    from .virtual_sprawl import prepare_virtual_sprawl

    try:
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
        # Install all required packages and get their location (in rust)
        sprawl_root, sprawl_packages = prepare_virtual_sprawl(
            filename, extras, get_pep508_env()
        )

        # activate the virtual sprawl
        sys.meta_path.append(VirtualSprawlPathFinder(sprawl_root, sprawl_packages))
    except Exception as e:
        print("VIRTUAL SPRAWL ERROR", e)
    except BaseException as e:  # Rust panic
        print("VIRTUAL SPRAWL CRASH (RUST PANIC)", e)
