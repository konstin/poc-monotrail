#!/usr/bin/env python

import json
from pathlib import Path

import packaging.tags

if __name__ == "__main__":
    tags = sorted(str(i) for i in packaging.tags.sys_tags())
    with Path(__file__).parent.joinpath("cp38-ubuntu-20-04.json").open("w") as fp:
        json.dump(tags, fp, indent=4)
