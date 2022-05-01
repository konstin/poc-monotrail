from pathlib import Path


def get_bin() -> Path:
    musl_release_bin = get_root().joinpath(
        "target/x86_64-unknown-linux-musl/release/monotrail"
    )
    if musl_release_bin.is_file():
        musl_release_ctime = musl_release_bin.stat().st_ctime
    else:
        musl_release_ctime = 0
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

    if musl_release_ctime > release_ctime and musl_release_ctime > debug_ctime:
        print("Using musl release")
        bin = musl_release_bin
    elif release_ctime > debug_ctime:
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
