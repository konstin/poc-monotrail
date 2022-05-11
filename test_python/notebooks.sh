#!/bin/bash

../.venv/bin/jupyter nbconvert --inplace --execute interactive.ipynb
../.venv/bin/jupyter nbconvert --clear-output --inplace --ClearMetadataPreprocessor.enabled=True interactive.ipynb
../.venv/bin/jupyter nbconvert --inplace --execute from_git.ipynb
../.venv/bin/jupyter nbconvert --clear-output --inplace --ClearMetadataPreprocessor.enabled=True from_git.ipynb
