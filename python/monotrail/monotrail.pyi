from pathlib import Path
from typing import Tuple, List, Optional, Dict, Union

project_name: str

class InstalledPackage:
    name: str
    python_version: str
    unique_version: str
    tag: str

    def monotrail_location(self, sprawl_root: Union[str, Path]) -> str: ...
    def monotrail_site_packages(
        self, sprawl_root: Union[str, Path], python_version: (int, int)
    ) -> str: ...

class Script:
    script_name: str
    module: str
    function: str

class FinderData:
    """The packaging and import data that is resolved by the rust part and deployed by
    the finder"""

    # The location where all packages are installed
    sprawl_root: str
    # All resolved and installed packages indexed by name
    sprawl_packages: List[InstalledPackage]
    # Given a module name, where's the corresponding module file and what are the
    # submodule_search_locations?
    spec_paths: Dict[str, Tuple[Optional[str], List[str]]]
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
    # The scripts in pyproject.toml
    root_scripts: Dict[str, Script]

class InjectData:
    """The FinderData is made by the installation system, the other fields are made by
    the inject system"""

    # The location of packages and imports
    finder_data: FinderData
    # For some reason on windows the location of the monotrail containing folder gets
    # inserted into `sys.path` so we need to remove it manually
    sys_path_removes: List[str]
    # Windows for some reason ignores `Py_SetProgramName`, so we need to set
    # `sys.executable` manually
    sys_executable: str

def monotrail_from_args(args: List[str]) -> FinderData: ...
def monotrail_from_requested(requested: str, lockfile: Optional[str]) -> FinderData: ...
def monotrail_from_git(
    repo_url: str, revision: str, extras: Optional[List[str]], lockfile: Optional[str]
) -> FinderData: ...
def monotrail_from_dir(dir: str, extras: List[str]) -> FinderData: ...
def monotrail_spec_paths(
    sprawl_root: str, sprawl_packages: List[InstalledPackage]
) -> Tuple[Dict[str, Tuple[str, List[str]]], List[str]]: ...
def monotrail_find_scripts(
    sprawl_root: str, sprawl_packages: List[InstalledPackage]
) -> Dict[str, str]: ...
