import logging
import site
import sys
from importlib.abc import MetaPathFinder
from importlib.machinery import PathFinder
from importlib.util import spec_from_file_location
from pathlib import Path
from typing import Union, List, Dict, Optional, Tuple

import importlib_metadata

# setuptools is adamant on haying _normalized_name on PathDistributions but as of 3.8 that only exists in
# importlib_metadata (or setuptools vendored copy of it) and not in importlib.metadata
# noinspection PyProtectedMember
from importlib_metadata import (
    DistributionFinder,
    PathDistribution,
    PackageNotFoundError,
)

from .monotrail import InstalledPackage, monotrail_spec_paths, project_name

logger = logging.getLogger(__name__)

# exploits that modules are only loaded once
_path_finder_singleton: Optional["MonotrailPathFinder"] = None


class MonotrailPathFinder(PathFinder, MetaPathFinder):
    # The location where all packages are installed
    sprawl_root: Path
    # All resolved and installed packages indexed by name
    sprawl_packages: Dict[str, InstalledPackage]
    # Given a module name, where's the corresponding module file and what are the submodule_search_locations?
    spec_paths: Dict[str, Tuple[str, List[str]]]
    # For from git, where we check our a repository and make it available for import
    repo_dir: Optional[str]

    def __init__(self):
        """dummy, actual initializer is update_and_activate"""
        # TODO: This dummy is unsound typing-wise. First init should always also set path and packages
        self.sprawl_packages = {}

    @staticmethod
    def get_singleton() -> "MonotrailPathFinder":
        """We want only one monotrail path finder to be active at any given time"""
        global _path_finder_singleton
        if not _path_finder_singleton:
            _path_finder_singleton = MonotrailPathFinder()
            sys.meta_path.append(_path_finder_singleton)
        return _path_finder_singleton

    def update_and_activate(
        self,
        sprawl_root: Union[str, Path],
        sprawl_packages: List[InstalledPackage],
        repo_dir: Optional[str] = None
    ):
        """Update the set of installed/available packages on the fly"""
        self.sprawl_root = Path(sprawl_root)
        self.warn_on_conflicts(self.sprawl_packages, sprawl_packages)
        self.sprawl_packages = {package.name: package for package in sprawl_packages}
        self.repo_dir=repo_dir
        self.spec_paths, pth_files = monotrail_spec_paths(sprawl_root, sprawl_packages)
        # hackery hack hack
        # we need to run .pth files because some project such as matplotlib 3.5.1 use them to commit packaging crimes
        for pth in pth_files:
            pth = Path(pth)
            site.addpackage(pth.parent, pth.name, None)

    @classmethod
    def warn_on_conflicts(
        cls,
        existing_packages: Dict[str, InstalledPackage],
        new_packages: List[InstalledPackage],
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

    def _single_distribution(self, package: InstalledPackage) -> PathDistribution:
        # TODO: Don't glob, but somehow handle that package can use the non-canonical name here
        [dist_info_dir] = Path(
            package.monotrail_site_packages(
                self.sprawl_root, (sys.version_info.major, sys.version_info.minor)
            )
        ).glob(f"*-{package.python_version}.dist-info")
        assert dist_info_dir.is_dir(), dist_info_dir
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
