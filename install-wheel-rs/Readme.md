Reimplementation of wheel installing in rust. Supports both classical venvs and monotrail.

There are python bindings, which currently don't really manage the whole locking-and-parallelism part, and there's only one function: `install_wheels_venv(wheels: List[str], venv: str)`, where `wheels` is a list of paths to wheel files and `venv` is the location of the venv to install the packages in.
