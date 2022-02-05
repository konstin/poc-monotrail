from subprocess import check_call

from test.compare import compare_installer, get_bin, get_root
from test.test_piptests import test_piptests
from test.test_popular import test_popular
from test.test_tqdm import test_tqdm


def main():
    check_call(["cargo", "build", "--release"])
    purelib_platlib_wheel = get_root().joinpath(
        "wheels/purelib_and_platlib-1.0.0-cp38-cp38-linux_x86_64.whl"
    )
    compare_installer("purelib_platlib", [purelib_platlib_wheel], get_bin())
    test_piptests()
    test_popular()
    test_tqdm()


if __name__ == "__main__":
    main()
