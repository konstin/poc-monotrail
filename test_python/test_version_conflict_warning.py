import io
from contextlib import redirect_stdout, redirect_stderr

# noinspection PyUnresolvedReferences
import monotrail


# noinspection PyUnresolvedReferences
def main():
    stdout_io = io.StringIO()
    stderr_io = io.StringIO()
    with redirect_stdout(stdout_io), redirect_stderr(stderr_io):
        monotrail.interactive(tqdm="4.62.0")
        # Not a problem, tqdm is not yet loaded
        monotrail.interactive(tqdm="4.63.0")
        import tqdm

        # Now it's a problem
        monotrail.interactive(tqdm="4.64.0")
    stdout_str = stdout_io.getvalue()
    stderr_str = stderr_io.getvalue()
    assert stdout_str.strip() == ""
    assert (
        stderr_str.strip()
        == "Version conflict: tqdm 4.63.0 is loaded, but 4.64.0 is now required. "
        "Please restart your jupyter kernel or python interpreter"
    )


if __name__ == "__main__":
    main()
