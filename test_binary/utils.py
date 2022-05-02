from pathlib import Path


def get_bin() -> Path:
    release_bin = get_root().joinpath("target/release/monotrail")
    if release_bin.is_file():
        release_ctime = release_bin.stat().st_ctime
    else:
        release_ctime = 0
    debug_bin = get_root().joinpath("target/debug/monotrail")
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
    return Path(__file__).parent.parent


if __name__ == "__main__":
    print(get_root())
    print(get_bin())
