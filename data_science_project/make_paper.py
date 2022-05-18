#!/usr/bin/env python
from pathlib import Path

import matplotlib.pyplot as plt
import numpy
import pandas

print(__file__)
current = Path(__file__).parent
iris = pandas.read_csv(current.joinpath("iris.csv"))
plt.hist(iris["sepal_length"], density=True)
plt.hist(numpy.random.normal(6, size=(1000,)), histtype="step", density=True)
plt.savefig(current.joinpath("iris.png"))
