def test_namespace_init_py(pytestconfig):
    # noinspection PyUnresolvedReferences
    from monotrail.monotrail import monotrail_from_dir

    poetry_self_toml_dir = pytestconfig.rootpath.joinpath("test-data/poetry-1.1.13")
    finder_data = monotrail_from_dir(poetry_self_toml_dir, [])
    assert finder_data.spec_paths.pop("poetry") == (
        finder_data.sprawl_root
        + "/poetry/1.1.13/py2.py3-none-any/lib/python/site-packages/poetry/__init__.py",
        [
            finder_data.sprawl_root
            + "/poetry/1.1.13/py2.py3-none-any/lib/python/site-packages/poetry",
            finder_data.sprawl_root
            + "/poetry_core/1.0.8/py2.py3-none-any/lib/python/site-packages/poetry",
        ],
    )

    for name, (_, submodule_search_locations) in finder_data.spec_paths.items():
        if name == "poetry" or name == "__pycache__":
            continue
        assert len(submodule_search_locations) <= 1, name


def test_namespace_no_init_py(pytestconfig):
    # noinspection PyUnresolvedReferences
    from monotrail.monotrail import monotrail_from_dir

    poetry_self_toml_dir = pytestconfig.rootpath.joinpath("test-data/poetry-1.2.0b1")
    finder_data = monotrail_from_dir(poetry_self_toml_dir, [])
    assert finder_data.spec_paths.pop("poetry") == (
        finder_data.sprawl_root
        + "/poetry/1.2.0b1/py3-none-any/lib/python/site-packages/poetry/__init__.py",
        [
            finder_data.sprawl_root
            + "/poetry/1.2.0b1/py3-none-any/lib/python/site-packages/poetry",
            finder_data.sprawl_root
            + "/poetry_core/1.1.0b2/py3-none-any/lib/python/site-packages/poetry",
        ],
    )

    for name, (_, submodule_search_locations) in finder_data.spec_paths.items():
        if name == "poetry" or name == "__pycache__":
            continue
        assert len(submodule_search_locations) <= 1, name
