import numpy


def main():
    identity_sum = numpy.identity(3).sum()
    print(identity_sum)
    assert identity_sum == 3
