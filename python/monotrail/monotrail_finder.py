import importlib.metadata as importlib_metadata
import logging
import site
import sys
import typing
from importlib.abc import MetaPathFinder
from importlib.machinery import PathFinder
from importlib.metadata import (
    DistributionFinder,
    PackageNotFoundError,
    PathDistribution,
)
from importlib.util import spec_from_file_location
from pathlib import Path
from typing import List, Dict, Optional, Tuple

if typing.TYPE_CHECKING:
    from .monotrail import FinderData, InstalledPackage

# modularity fallback
try:
    from .monotrail import project_name
except (ModuleNotFoundError, ImportError):
    project_name = "monotrail"

logger = logging.getLogger(__name__)

# exploits that modules are only loaded once
_finder_singleton: Optional["MonotrailFinder"] = None


class MonotrailFinder(PathFinder, MetaPathFinder):
    # The location where all packages are installed
    sprawl_root: Path
    # All resolved and installed packages indexed by name
    sprawl_packages: Dict[str, "InstalledPackage"]
    # Given a module name, where's the corresponding module file and what are the submodule_search_locations?
    spec_paths: Dict[str, Tuple[str, List[str]]]
    # In from git mode where we check out a repository and make it available for import as if it was added to sys.path
    repo_dir: Optional[str]
    # The contents of the last poetry.lock, used a basis for the next resolution when requirements
    # change at runtime, both for faster resolution and in hopes the exact version stay the same
    # so the user doesn't need to reload python
    lockfile: Optional[str]

    def __init__(self):
        """dummy, actual initializer is update_and_activate"""
        # TODO: This dummy is unsound typing-wise. First init should always also set path and packages
        self.sprawl_packages = {}
        self.spec_paths = {}
        self.repo_dir = None
        self.lockfile = None

    @staticmethod
    def get_singleton() -> "MonotrailFinder":
        """We want only one monotrail finder to be active at any given time"""
        global _finder_singleton
        if not _finder_singleton:
            _finder_singleton = MonotrailFinder()
            sys.meta_path.append(_finder_singleton)
        return _finder_singleton

    def update_and_activate(self, finder_data: "FinderData"):
        """Update the set of installed/available packages on the fly"""
        self.sprawl_root = Path(finder_data.sprawl_root)
        self.warn_on_conflicts(self.sprawl_packages, finder_data.sprawl_packages)
        self.sprawl_packages = {
            package.name: package for package in finder_data.sprawl_packages
        }
        self.repo_dir = finder_data.repo_dir
        self.spec_paths = finder_data.spec_paths
        # hackery hack hack
        # we need to run .pth files because some project such as matplotlib 3.5.1 use them to commit packaging crimes
        for pth in finder_data.pth_files:
            pth = Path(pth)
            site.addpackage(pth.parent, pth.name, None)

    @classmethod
    def warn_on_conflicts(
        cls,
        existing_packages: Dict[str, "InstalledPackage"],
        new_packages: List["InstalledPackage"],
    ):
        """if we already have a different version loaded that version will stay loaded, so we have a conflict,
        and the only solution is really to restart python. We could remove it from sys.modules, but anything
        already imported from the module will stay loaded and cause strange undebuggable conflicts
        """
        imported_versions = {}
        # copy the dict size will otherwise change during iteration
        for module in list(sys.modules.keys()):
            if "." in module:
                continue
            try:
                imported_versions[module] = importlib_metadata.version(module)
            except PackageNotFoundError:
                # e.g. builtin modules such as sys and os, but also broken stuff
                imported_versions[module] = "missing_metadata"

        for new in new_packages:
            # not yet loaded, not a problem
            if new.name not in imported_versions:
                continue
            imported_version = imported_versions[new.name]
            existing = existing_packages.get(new.name)
            if existing:
                if new.unique_version != existing.unique_version:
                    logger.warning(
                        f"Version conflict: {existing.name} {existing.unique_version} is loaded, "
                        f"but {new.unique_version} is now required. "
                        f"Please restart your jupyter kernel or python interpreter",
                    )
            else:
                if imported_version != new.unique_version:
                    logger.warning(
                        f"Version conflict: {new.name} {imported_version} was already imported, "
                        f"even though {new.unique_version} is now required. "
                        f"Is there any other loading mechanism with higher precedence than {project_name}?",
                    )

    def find_spec(self, fullname, path=None, target=None):
        # We need to pass all packages because package names are lies, packages may contain whatever and nobody uses
        # https://packaging.python.org/en/latest/specifications/core-metadata/#provides-dist-multiple-use
        # e.g. "python-dateutil" actually ships a module "dateutil" but there's no indication about that

        if fullname in self.spec_paths:
            location, submodule_search_locations = self.spec_paths[fullname]
        elif self.repo_dir:
            # Maybe we can find it in the repository we checked out
            # It would make more sense to only add the poetry known modules, but this behaviour is most likely what
            # users expect
            return super().find_spec(fullname, [self.repo_dir])
        else:
            return None

        spec = spec_from_file_location(
            fullname,
            location,
            submodule_search_locations=submodule_search_locations,
        )

        return spec

    def _single_distribution(self, package: "InstalledPackage") -> PathDistribution:
        # TODO: Don't glob, but somehow handle that package can use the non-canonical name here
        site_packages = Path(
            package.monotrail_site_packages(
                self.sprawl_root, (sys.version_info.major, sys.version_info.minor)
            )
        )
        dist_info_dirs = list(
            site_packages.glob(f"*-{package.python_version}.dist-info")
        )
        if not len(dist_info_dirs) == 1:
            raise RuntimeError(
                f"Failed to find {site_packages}/*-{package.python_version}.dist-info"
            )
        [dist_info_dir] = dist_info_dirs
        assert dist_info_dir.is_dir(), f"Not a directory: {dist_info_dir}"

        # https://github.com/pypa/setuptools/issues/3319
        # setuptools is adamant on haying _normalized_name on PathDistributions but as of 3.8 that only exists in
        # importlib_metadata. Specifically, it wants to use functions only available in its own vendored
        # importlib_metadata and not in importlib.metadata.
        # hacky fixup: If setuptools is the caller, the below import will also work and we'll return the thing it wants.
        # Other tools are also fine with importlib_metadata so far.
        try:
            # noinspection PyUnresolvedReferences,PyProtectedMember
            from setuptools._vendor.importlib_metadata import PathDistribution
        except ModuleNotFoundError:
            pass
        distribution = PathDistribution(dist_info_dir)
        return distribution

    def find_distributions(
        self, context: DistributionFinder.Context = DistributionFinder.Context()
    ):
        """https://docs.python.org/3/library/importlib.metadata.html#extending-the-search-algorithm

        Essentially, context has a name and a path attribute and we need to return an iterator with
        our Distribution object"""
        if context.name in self.sprawl_packages:
            package = self.sprawl_packages[context.name]
            return iter([self._single_distribution(package)])
        elif context.name is None:
            # return all packages, this is used e.g. by pytest -> pluggy for plugin discovery
            return (
                self._single_distribution(package)
                for package in self.sprawl_packages.values()
            )
        else:
            return iter([])
