from typing import Tuple, List, Optional

class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

def prepare_monotrail(
    script: Optional[str], extras: List[str], pep508_env: str
) -> Tuple[str, List[InstalledPackage]]: ...
def prepare_monotrail_from_env(args: List[str]) -> Tuple[str, List[InstalledPackage]]: ...
