import sys

version = sys.version.replace("\n", "")
print(f"Hello from Python {version}")
import numpy

print(f"loaded numpy {numpy.__version__} from {numpy.__file__}")
