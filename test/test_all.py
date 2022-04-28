from subprocess import check_call


def main():
    # don't confuse pytest
    from test.test_compare_pip import test_purelib_platlib
    from test.test_compare_poetry import test_data_science_project
    from test.test_piptests import test_piptests
    from test.test_popular import test_popular
    from test.test_tqdm import test_tqdm

    # build release only with maturin because cargo caching breaks when we switch linker options
    check_call("cargo build --release --target x86_64-unknown-linux-musl".split(" "))
    test_purelib_platlib()
    test_piptests()
    test_popular()
    test_data_science_project()
    test_tqdm()


if __name__ == "__main__":
    main()
