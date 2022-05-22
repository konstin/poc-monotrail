"""Manually reimplementing some bridging code. This could be much more elegant by exporting type from the rust binary
to python"""
import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Tuple, List, Optional, Dict, Union


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
        return str(
            Path(self.monotrail_location(sprawl_root))
            .joinpath("lib")
            # .joinpath(f"python{python_version[0]}.{python_version[1]}")
            .joinpath(f"python")
            .joinpath("site-packages")
        )


@dataclass
class FinderData:
    """The packaging and import data that is resolved by the rust part and deployed by the finder"""

    # The location where all packages are installed
    sprawl_root: str
    # All resolved and installed packages indexed by name
    sprawl_packages: List[InstalledPackage]
    # Given a module name, where's the corresponding module file and what are the submodule_search_locations?
    spec_paths: Dict[str, Tuple[str, List[str]]]
    # In from git mode where we check out a repository and make it available for import as if it was added to sys.path
    repo_dir: Optional[str]
    # we need to run .pth files because some project such as matplotlib 3.5.1 use them to commit packaging crimes
    pth_files: List[str]
    # The contents of the last poetry.lock, used a basis for the next resolution when requirements
    # change at runtime, both for faster resolution and in hopes the exact version stay the same
    # so the user doesn't need to reload python
    lockfile: Optional[str]
    # The installed scripts indexed by name. They are in the bin folder of each project, coming
    # from entry_points.txt or data folder scripts
    scripts: Dict[str, str]

    @classmethod
    def from_json(cls, data: str) -> "FinderData":
        data = json.loads(data)
        data["sprawl_packages"] = [
            InstalledPackage(**i) for i in data["sprawl_packages"]
        ]
        return cls(**data)


def maybe_debug():
    """delayed until with have the package"""
    if os.environ.get("PYCHARM_REMOTE_DEBUG"):
        # noinspection PyUnresolvedReferences
        import pydevd_pycharm

        port = int(os.environ["PYCHARM_REMOTE_DEBUG"])
        pydevd_pycharm.settrace("localhost", port=port, suspend=False)
