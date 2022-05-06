#!/bin/bash

../.venv/bin/jupyter nbconvert --inplace --execute jupyter_integration.ipynb
../.venv/bin/jupyter nbconvert --clear-output --inplace --ClearMetadataPreprocessor.enabled=True jupyter_integration.ipynb
