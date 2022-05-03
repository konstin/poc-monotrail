from typing import Tuple, List, Optional


class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

def monotrail_from_env(
    args: List[str],
) -> Tuple[str, List[InstalledPackage]]: ...
def monotrail_from_requested(
    requested: str, lockfile: Optional[str]
) -> Tuple[str, List[InstalledPackage], str]: ...
