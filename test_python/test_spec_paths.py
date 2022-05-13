def test_spec_paths(pytestconfig):
    # noinspection PyUnresolvedReferences
    from monotrail.monotrail import monotrail_spec_paths, monotrail_from_dir

    poetry_self_toml_dir = pytestconfig.rootpath.joinpath(
        "src/poetry_integration/poetry_boostrap_lock"
    )
    finder_data = monotrail_from_dir(poetry_self_toml_dir, [])
    assert finder_data.spec_paths.pop("poetry") == (
        finder_data.sprawl_root
        + "/poetry/1.1.13/py2.py3-none-any/lib/python3.8/site-packages/poetry/__init__.py",
        [
            finder_data.sprawl_root
            + "/poetry/1.1.13/py2.py3-none-any/lib/python3.8/site-packages/poetry",
            finder_data.sprawl_root
            + "/poetry_core/1.0.8/py2.py3-none-any/lib/python3.8/site-packages/poetry",
        ],
    )

    for name, (_, submodule_search_locations) in finder_data.spec_paths.items():
        if name == "poetry":
            continue
        assert len(submodule_search_locations) <= 1, name
