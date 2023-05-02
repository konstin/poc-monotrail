Test project for testing when pip install locally vs from pypi.

 * `-e numpy`: locally
 * `-e ./numpy`: locally
 * `numpy`: index
 * `./numpy`: locally

Works:
```shell
test-data/requirements-txt-ambiguity/
.venv/bin/pip uninstall -y numpy
.venv/bin/pip install --no-cache-dir -r requirements-ambiguous.txt)
```
Fails:
```shell
test-data/requirements-txt-ambiguity/.venv/bin/pip uninstall -y numpy
test-data/requirements-txt-ambiguity/.venv/bin/pip install --no-cache-dir -r test-data/requirements-txt-ambiguity/requirements-txt-ambiguous.txt
```