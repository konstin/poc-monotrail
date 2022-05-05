from pathlib import Path


def test_spec_paths():
    # noinspection PyUnresolvedReferences
    from monotrail.monotrail import monotrail_spec_paths, monotrail_from_dir

    poetry_self_toml_dir = Path(__file__).parent.parent.joinpath(
        "src/poetry/poetry_boostrap_lock"
    )
    sprawl_root, sprawl_packages = monotrail_from_dir(poetry_self_toml_dir, [])
    spec_paths, _pths = monotrail_spec_paths(sprawl_root, sprawl_packages)
    assert spec_paths.pop("poetry") == (
        sprawl_root
        + "/poetry/1.1.13/py2.py3-none-any/lib/python3.8/site-packages/poetry/__init__.py",
        [
            sprawl_root
            + "/poetry/1.1.13/py2.py3-none-any/lib/python3.8/site-packages/poetry",
            sprawl_root
            + "/poetry_core/1.0.8/py2.py3-none-any/lib/python3.8/site-packages/poetry",
        ],
    )

    for name, (_, submodule_search_locations) in spec_paths.items():
        assert len(submodule_search_locations) <= 1, name
