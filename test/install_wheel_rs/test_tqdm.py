import os
import platform
import shutil
from subprocess import check_call, SubprocessError, DEVNULL, CalledProcessError

from test.install_wheel_rs.utils import get_bin, get_root


def test_tqdm():
    venv = get_root().joinpath("test-venvs").joinpath("venv-tqdm")
    if venv.is_dir():
        shutil.rmtree(venv)
    check_call(["virtualenv", "-p", "3.8", venv])
    env = {**os.environ, "VIRTUAL_ENV": str(venv)}

    if platform.system() == "Windows":
        python = venv.joinpath("Scripts").joinpath("python.exe")
        tqdm = venv.joinpath("Scripts").joinpath("tqdm.exe")
    else:
        python = venv.joinpath("bin").joinpath("python")
        tqdm = venv.joinpath("bin").joinpath("tqdm")

    try:
        check_call(
            [python, "tqdm_test.py"],
            env=env,
            stdout=DEVNULL,
            stderr=DEVNULL,
        )
        raise AssertionError()
    except SubprocessError:
        pass
    try:
        check_call([python, tqdm, "--version"], env=env, stdout=DEVNULL, stderr=DEVNULL)
        raise AssertionError()
    except CalledProcessError:
        pass

    tqdm_wheel = (
        get_root()
        .joinpath("test-data")
        .joinpath("popular-wheels")
        .joinpath("tqdm-4.62.3-py2.py3-none-any.whl")
    )
    check_call([get_bin("monotrail"), "wheel-install", tqdm_wheel], env=env)
    check_call(
        [
            python,
            get_root()
            .joinpath("test")
            .joinpath("install_wheel_rs")
            .joinpath("test_tqdm_impl.py"),
        ],
        env=env,
    )
    check_call([python, tqdm, "--version"], env=env)
    shutil.rmtree(venv)


if __name__ == "__main__":
    test_tqdm()
