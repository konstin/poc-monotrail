import os
import shutil
from pathlib import Path
from subprocess import check_call, SubprocessError, DEVNULL


def main():
    venv = Path(__file__).parent.parent.joinpath("../.venv-tqdm")
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

    check_call(
        [
            "target/release/install-wheel-rs",
            "install-files",
            "wheels/tqdm-4.62.3-py2.py3-none-any.whl",
        ],
        env=env,
    )
    check_call([f"{venv}/bin/python", "test/tqdm_test.py"], env=env)
    check_call([f"{venv}/bin/tqdm", "--version"], env=env)
    shutil.rmtree(venv)


if __name__ == "__main__":
    main()
