from typing import Tuple, List, Optional

class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

def prepare_monorail(
    file_running: Optional[str], extras: List[str], pep508_env: str
) -> Tuple[str, List[InstalledPackage]]: ...
