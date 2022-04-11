import os

if os.environ.get("VIRTUAL_SPRAWL"):
    import sys
    from importlib.abc import MetaPathFinder
    from importlib.machinery import PathFinder
    from pathlib import Path
    from typing import Union, List, Tuple
    from .get_pep508_env import get_bindings

    # noinspection PyUnresolvedReferences
    from .virtual_sprawl import get_virtual_sprawl_info

    try:
        sprawl_root, sprawl_packages = get_virtual_sprawl_info(
            sys.argv[0], get_bindings()
        )

        class MyPathFinder(PathFinder, MetaPathFinder):
            def __init__(
                self,
                sprawl_root: Union[str, Path],
                sprawl_packages: List[Tuple[str, str]],
            ):
                self.sprawl_root = Path(sprawl_root)
                self.sprawl_packages = {
                    name: (name, version) for name, version in sprawl_packages
                }

            def find_spec(self, fullname, path=None, target=None):
                # We need to pass all packages because package names are lies and they may contain whatever, and nobody uses
                # https://packaging.python.org/en/latest/specifications/core-metadata/#provides-dist-multiple-use
                # e.g. "python-dateutil" actually ships a module "dateutil" but there's no indication about that
                site_packages = [
                    str(
                        self.sprawl_root.joinpath(name + "-" + version)
                        .joinpath("lib")
                        .joinpath(
                            f"python{sys.version_info.major}.{sys.version_info.minor}"
                        )
                        .joinpath("site-packages")
                    )
                    for name, version in self.sprawl_packages.values()
                ]
                return super().find_spec(fullname, site_packages, target)

        sys.meta_path.append(MyPathFinder(sprawl_root, sprawl_packages))
    except Exception as e:
        print("VIRTUAL SPRAWL ERROR", e)
    except BaseException as e:  # Rust panic
        print("VIRTUAL SPRAWL CRASH (RUST PANIC)", e)
