from typing import Tuple, List

class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

def prepare_virtual_sprawl(
    file_running: str, extras: List[str], pep508_env: str
) -> Tuple[str, List[InstalledPackage]]: ...
