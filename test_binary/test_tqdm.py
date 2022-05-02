import os
import shutil
from pathlib import Path
from subprocess import check_call, SubprocessError, DEVNULL

from test_binary.utils import get_bin, get_root


def test_tqdm():
    venv = Path(__file__).parent.parent.joinpath("test-venvs/venv-tqdm")
    if venv.is_dir():
        shutil.rmtree(venv)
    check_call(["virtualenv", venv])
    env = {**os.environ, "VIRTUAL_ENV": venv}

    try:
        check_call(
            [f"{venv}/bin/python", "tqdm_test.py"],
            env=env,
            stdout=DEVNULL,
            stderr=DEVNULL,
        )
        assert False
    except SubprocessError:
        pass
    try:
        check_call(
            [f"{venv}/bin/tqdm", "--version"], env=env, stdout=DEVNULL, stderr=DEVNULL
        )
        assert False
    except FileNotFoundError:
        pass

    tqdm_wheel = get_root().joinpath("popular-wheels/tqdm-4.62.3-py2.py3-none-any.whl")
    check_call(
        [get_bin(), "install", tqdm_wheel],
        env=env,
    )
    check_call(
        [f"{venv}/bin/python", get_root().joinpath("test_binary/test_tqdm_impl.py")], env=env
    )
    check_call([f"{venv}/bin/tqdm", "--version"], env=env)
    shutil.rmtree(venv)


if __name__ == "__main__":
    test_tqdm()
