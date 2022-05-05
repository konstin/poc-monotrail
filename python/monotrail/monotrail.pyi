from pathlib import Path
from typing import Tuple, List, Optional, Dict, Union

NAME = "MONOTRAIL"

class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

    def monotrail_location(self, sprawl_root: Union[str, Path]) -> str: ...
    def monotrail_site_packages(
        self, sprawl_root: Union[str, Path], python_version: (int, int)
    ) -> str: ...

def monotrail_from_env(
    args: List[str],
) -> Tuple[str, List[InstalledPackage]]: ...
def monotrail_from_requested(
    requested: str, lockfile: Optional[str]
) -> Tuple[str, List[InstalledPackage], str]: ...
def monotrail_spec_paths(
    sprawl_root: str, sprawl_packages: List[InstalledPackage]
) -> Tuple[Dict[str, Tuple[str, List[str]]], List[str]]: ...
