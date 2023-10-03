from pathlib import Path

from test.install_wheel_rs.utils import get_bin, compare_with_pip_args


def test_pydantic(pytestconfig):
    root = pytestconfig.rootpath
    requirements_all_txt = "test-data/requirements-pydantic/all.txt"
    # source distributions, the contents of these lines depend on the build env
    content_diff_exclusions = {
        Path("lib/python3.8/site-packages/mkdocs_exclude-1.0.2.dist-info/METADATA"),
        Path("lib/python3.8/site-packages/mkdocs_exclude-1.0.2.dist-info/WHEEL"),
        Path(
            "lib/python3.8/site-packages/mkdocs_exclude-1.0.2.dist-info/entry_points.txt"
        ),
        Path("lib/python3.8/site-packages/mkdocs_redirects-1.2.0.dist-info/METADATA"),
        Path("lib/python3.8/site-packages/mkdocs_redirects-1.2.0.dist-info/WHEEL"),
        Path(
            "lib/python3.8/site-packages/mkdocs_redirects-1.2.0.dist-info/entry_points.txt"
        ),
    }
    compare_with_pip_args(
        bin_name=get_bin("monotrail"),
        env_name="monotrail_install_frozen_pydantic",
        pip_args=["--no-deps", "--no-compile", "-r", requirements_all_txt],
        monotrail_args=["install", "--frozen", "-r", requirements_all_txt],
        cwd=root,
        content_diff_exclusions=content_diff_exclusions,
    )
