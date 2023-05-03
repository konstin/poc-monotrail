from test.install.test_compare_pip import compare_with_pip_args


def test_pydantic(pytestconfig):
    root = pytestconfig.rootpath
    requirements_all_txt = "test-data/requirements-pydantic/all.txt"
    compare_with_pip_args(
        env_name="monotrail_install_frozen_pydantic",
        pip_args=["--no-deps", "--no-compile", "-r", requirements_all_txt],
        monotrail_args=["install", "--frozen", "-r", requirements_all_txt],
        cwd=root,
    )
