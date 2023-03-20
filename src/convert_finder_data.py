"""
Manually reimplementing some bridging code. This could be much more elegant by exporting
the types from the rust binary to python, which would require getting pyo3-ffi to work
with libloading.
"""

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Tuple, List, Optional, Dict, Union, Any


@dataclass
class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

    def monotrail_location(self, sprawl_root: Union[str, Path]) -> str:
        return str(
            Path(sprawl_root)
            .joinpath(self.name)
            .joinpath(self.unique_version)
            .joinpath(self.tag)
        )

    def monotrail_site_packages(
        self,
        sprawl_root: Union[str, Path],
        # keep python version around in case we'll need to refactor to use it again
        _python_version: (int, int),
    ) -> str:
        if os.name == "nt":
            return str(
                Path(self.monotrail_location(sprawl_root))
                .joinpath("Lib")
                .joinpath("site-packages")
            )
        else:
            return str(
                Path(self.monotrail_location(sprawl_root))
                .joinpath("lib")
                # .joinpath(f"python{python_version[0]}.{python_version[1]}")
                .joinpath("python")
                .joinpath("site-packages")
            )


@dataclass
class Script:
    script_name: str
    module: str
    function: str


@dataclass
class FinderData:
    """The packaging and import data that is resolved by the rust part and deployed by
    the finder"""

    # The location where all packages are installed
    sprawl_root: str
    # All resolved and installed packages indexed by name
    sprawl_packages: List[InstalledPackage]
    # Given a module name, where's the corresponding module file and what are the
    # submodule_search_locations?
    spec_paths: Dict[str, Tuple[str, List[str]]]
    # In from git mode where we check out a repository and make it available for import
    # as if it was added to sys.path
    project_dir: Optional[str]
    # we need to run .pth files because some project such as matplotlib 3.5.1 use them
    # to commit packaging crimes
    pth_files: List[str]
    # The contents of the last poetry.lock, used a basis for the next resolution when
    # requirements change at runtime, both for faster resolution and in hopes the exact
    # version stay the same so the user doesn't need to reload python
    lockfile: Optional[str]
    # The scripts from pyproject.toml
    root_scripts: Dict[str, Any]
    # For some reason on windows the location of the monotrail containing folder gets
    # inserted into `sys.path` so we need to remove it manually
    sys_path_removes: List[str]

    @classmethod
    def from_json(cls, data: str) -> "FinderData":
        data = json.loads(data)
        data["sprawl_packages"] = [
            InstalledPackage(**i) for i in data["sprawl_packages"]
        ]
        data["root_scripts"] = {
            name: Script(**value) for name, value in data["root_scripts"].items()
        }
        return cls(**data)


def maybe_debug():
    """delayed until with have the package"""
    if os.environ.get("PYCHARM_REMOTE_DEBUG"):
        # noinspection PyUnresolvedReferences
        import pydevd_pycharm

        port = int(os.environ["PYCHARM_REMOTE_DEBUG"])
        pydevd_pycharm.settrace("localhost", port=port, suspend=False)
