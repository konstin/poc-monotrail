import sys
from importlib.abc import MetaPathFinder
from importlib.machinery import PathFinder
from importlib.metadata import DistributionFinder, PathDistribution
from pathlib import Path
from typing import Union, List, Dict

from .virtual_sprawl import InstalledPackage


class VirtualSprawlPathFinder(PathFinder, MetaPathFinder):
    sprawl_root: Path
    sprawl_packages: Dict[str, InstalledPackage]

    def __init__(
        self,
        sprawl_root: Union[str, Path],
        sprawl_packages: List[InstalledPackage],
    ):
        self.sprawl_root = Path(sprawl_root)
        self.sprawl_packages = {package.name: package for package in sprawl_packages}

    def _site_package_dir(self, name: str, unique_version: str) -> Path:
        return (
            self.sprawl_root.joinpath(name + "-" + unique_version)
            .joinpath("lib")
            .joinpath(f"python{sys.version_info.major}.{sys.version_info.minor}")
            .joinpath("site-packages")
        )

    def find_spec(self, fullname, path=None, target=None):
        # We need to pass all packages because package names are lies, packages may contain whatever and nobody uses
        # https://packaging.python.org/en/latest/specifications/core-metadata/#provides-dist-multiple-use
        # e.g. "python-dateutil" actually ships a module "dateutil" but there's no indication about that
        site_packages = []
        for package in self.sprawl_packages.values():
            site_packages_dir = self._site_package_dir(
                package.name, package.unique_version
            )
            assert (
                site_packages_dir.is_dir()
            ), f"missing expected directory: {site_packages_dir}"
            site_packages.append(str(site_packages_dir))
        return super().find_spec(fullname, site_packages, target)

    def find_distributions(
        self, context: DistributionFinder.Context = DistributionFinder.Context()
    ):
        """https://docs.python.org/3/library/importlib.metadata.html#extending-the-search-algorithm

        Essentially, context has a name and a path attribute and we need to return an iterator with
        our Distribution object"""
        if context.name in self.sprawl_packages:
            package = self.sprawl_packages[context.name]
            dist_info_dir = self._site_package_dir(
                package.name, package.unique_version
            ).joinpath(f"{package.name}-{package.python_version}.dist-info")
            return iter([PathDistribution(dist_info_dir)])
        else:
            return iter([])
