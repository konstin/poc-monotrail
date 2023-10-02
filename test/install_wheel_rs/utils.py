import platform
from pathlib import Path


def get_bin(bin_filename: str = "monotrail") -> Path:
    if platform.system() == "Windows":
        bin_filename = f"{bin_filename}.exe"

    release_bin = (
        get_root().joinpath("target").joinpath("release").joinpath(bin_filename)
    )
    if release_bin.is_file():
        release_ctime = release_bin.stat().st_ctime
    else:
        release_ctime = 0
    debug_bin = get_root().joinpath("target").joinpath("debug").joinpath(bin_filename)
    if debug_bin.is_file():
        debug_ctime = debug_bin.stat().st_ctime
    else:
        debug_ctime = 0

    if release_ctime > debug_ctime:
        print("Using release")
        bin = release_bin
    else:
        print("Using debug")
        bin = debug_bin

    return bin


def get_root() -> Path:
    return Path(__file__).parent.parent.parent


if __name__ == "__main__":
    print(get_root())
    print(get_bin())
