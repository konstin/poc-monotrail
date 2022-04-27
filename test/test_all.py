from subprocess import check_call

from test.compare_pip import compare_with_pip, get_bin, get_root
from test.test_piptests import test_piptests
from test.test_popular import test_popular
from test.test_tqdm import test_tqdm


def main():
    # build release only with maturin because cargo caching breaks when we switch linker options
    check_call(["maturin", "build", "--release", "--strip", "-i", "python"])
    purelib_platlib_wheel = get_root().joinpath(
        "test-data/wheels/purelib_and_platlib-1.0.0-cp38-cp38-linux_x86_64.whl"
    )
    compare_with_pip("purelib_platlib", [purelib_platlib_wheel], get_bin())
    test_piptests()
    test_popular()
    test_tqdm()


if __name__ == "__main__":
    main()
