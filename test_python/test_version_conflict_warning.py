def test_version_conflict_warning(caplog):
    # noinspection PyUnresolvedReferences
    import monotrail

    monotrail.interactive(tqdm="4.62.0")
    # Not a problem, tqdm is not yet loaded
    monotrail.interactive(tqdm="4.63.0")
    # noinspection PyUnresolvedReferences
    import tqdm

    # Now it's a problem
    monotrail.interactive(tqdm="4.64.0")
    assert caplog.messages == [
        "Version conflict: tqdm 4.63.0 is loaded, but 4.64.0 is now required. "
        "Please restart your jupyter kernel or python interpreter"
    ]
