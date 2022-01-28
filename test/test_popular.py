#!/usr/bin/env python
import itertools
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from subprocess import check_call

from test.compare import compare_installer

wheels = [
    "aiohttp-3.8.1",
    "aiosignal-1.2.0",
    "asgiref-3.5.0",
    "async_timeout-4.0.2",
    "attrs-21.4.0",
    "awscli-1.22.46",
    "azure_common-1.1.27",
    "azure_core-1.21.1",
    "azure_storage_blob-12.9.0",
    "backports.zoneinfo-0.2.1",
    "beautifulsoup4-4.10.0",
    "boto3-1.20.46",
    "botocore-1.23.46",
    "cachetools-5.0.0",
    "certifi-2021.10.8",
    "cffi-1.15.0",
    "chardet-4.0.0",
    "charset_normalizer-2.0.11",
    "click-8.0.3",
    "colorama-0.4.3",
    "cryptography-36.0.1",
    "cycler-0.11.0",
    "decorator-5.1.1",
    "Django-4.0.1",
    "docker-5.0.3",
    "docutils-0.15.2",
    "filelock-3.4.2",
    "Flask-2.0.2",
    "fonttools-4.29.1",
    "frozenlist-1.3.0",
    "fsspec-2022.1.0",
    "gitdb-4.0.9",
    "GitPython-3.1.26",
    "google_api_core-2.4.0",
    "google_api_python_client-2.36.0",
    "google_auth-2.6.0",
    "google_auth_httplib2-0.1.0",
    "google_cloud_bigquery-2.32.0",
    "google_cloud_core-2.2.2",
    "google_cloud_storage-2.1.0",
    "google_crc32c-1.3.0",
    "google_resumable_media-2.1.0",
    "googleapis_common_protos-1.54.0",
    "greenlet-1.1.2",
    "grpcio-1.43.0",
    "grpcio_status-1.43.0",
    "gunicorn-20.1.0",
    "httplib2-0.20.2",
    "idna-3.3",
    "importlib_metadata-4.10.1",
    "importlib_resources-5.4.0",
    "iniconfig-1.1.1",
    "isodate-0.6.1",
    "itsdangerous-2.0.1",
    "Jinja2-3.0.3",
    "jmespath-0.10.0",
    "joblib-1.1.0",
    "jsonschema-4.4.0",
    "kiwisolver-1.3.2",
    "lxml-4.7.1",
    "MarkupSafe-2.0.1",
    "matplotlib-3.5.1",
    "msrest-0.6.21",
    "multidict-6.0.2",
    "mypy_extensions-0.4.3",
    "numpy-1.22.1",
    "oauthlib-3.2.0",
    "packaging-21.3",
    "pandas-1.4.0",
    "Pillow-9.0.0",
    "platformdirs-2.4.1",
    "plotly-5.5.0",
    "pluggy-1.0.0",
    "proto_plus-1.19.9",
    "protobuf-3.19.4",
    "psutil-5.9.0",
    # TODO: Bug in https://github.com/PyO3/python-pkginfo-rs/blob/6703a29b11cf427bcd2c6c255532c7473eaae0d6/src/distribution.rs#L225
    # "py-1.11.0",
    "py_spy-0.3.11",
    "pyarrow-6.0.1",
    "pyasn1-0.4.8",
    "pyasn1_modules-0.2.8",
    "pycparser-2.21",
    "Pygments-2.11.2",
    "PyJWT-2.3.0",
    "pyOpenSSL-22.0.0",
    "pyparsing-3.0.7",
    "pyrsistent-0.18.1",
    "pytest-6.2.5",
    "python_dateutil-2.8.2",
    "pytz-2021.3",
    "PyYAML-5.4.1",
    "requests-2.27.1",
    "requests_oauthlib-1.3.1",
    "rsa-4.7.2",
    "s3transfer-0.5.0",
    "scipy-1.7.3",
    "six-1.16.0",
    "smmap-5.0.0",
    "soupsieve-2.3.1",
    "SQLAlchemy-1.4.31",
    "sqlparse-0.4.2",
    "tabulate-0.8.9",
    "tenacity-8.0.1",
    "toml-0.10.2",
    "tqdm-4.62.3",
    "typing_extensions-4.0.1",
    "uritemplate-4.1.1",
    "urllib3-1.26.8",
    "websocket_client-1.2.3",
    "Werkzeug-2.0.2",
    "wrapt-1.13.3",
    "yarl-1.7.2",
    "zipp-3.7.0",
]

wheels_data = [
    "awscli-1.22.46",
    "docutils-0.15.2",
    "plotly-5.5.0",
    "py_spy-0.3.11",
]


def main():
    wheels_dir = Path(__file__).parent.parent.joinpath("wheels")
    if not wheels_dir.is_dir():
        print("Downloading wheels")
        check_call(
            [
                "pip",
                "download",
                "-d",
                wheels_dir,
                "-r",
                Path(__file__).parent.parent.joinpath("popular100.txt"),
            ]
        )
    release_bin = Path("target/release/install-wheel-rs")
    if release_bin.is_file():
        release_ctime = release_bin.stat().st_ctime
    else:
        release_ctime = 0
    debug_bin = Path("target/debug/install-wheel-rs")
    if debug_bin.is_file():
        debug_ctime = debug_bin.stat().st_ctime
    else:
        debug_ctime = 0

    if release_ctime > debug_ctime:
        print("Using release")
        bin = release_bin
    else:
        print("Using debug")
        bin = debug_bin

    with ThreadPoolExecutor() as executor:
        list(executor.map(compare_installer, wheels, itertools.repeat(bin)))


if __name__ == "__main__":
    main()
