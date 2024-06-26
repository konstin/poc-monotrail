"""
Taken from https://peps.python.org/pep-0508/#complete-grammar

passes the values back to rust as json
"""

import json
import os
import platform
import sys


def format_full_version(info):
    version = "{0.major}.{0.minor}.{0.micro}".format(info)
    kind = info.releaselevel
    if kind != "final":
        version += kind[0] + str(info.serial)
    return version


def get_pep508_env() -> str:
    if hasattr(sys, "implementation"):
        implementation_version = format_full_version(sys.implementation.version)
        implementation_name = sys.implementation.name
    else:
        implementation_version = "0"
        implementation_name = ""

    bindings = {
        "implementation_name": implementation_name,
        "implementation_version": implementation_version,
        "os_name": os.name,
        "platform_machine": platform.machine(),
        "platform_python_implementation": platform.python_implementation(),
        "platform_release": platform.release(),
        "platform_system": platform.system(),
        "platform_version": platform.version(),
        "python_full_version": platform.python_version(),
        "python_version": ".".join(platform.python_version_tuple()[:2]),
        "sys_platform": sys.platform,
    }
    return json.dumps(bindings)
