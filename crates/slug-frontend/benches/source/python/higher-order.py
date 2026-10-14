from functools import reduce


def score(values, scale, offset):
    mapped = list(map(lambda value: value * scale + offset, values))
    kept = list(filter(lambda value: value % 3 != 0, mapped))
    return reduce(lambda total, value: total + value % 1_000, kept, 0)


values = list(range(1, 2_001))
print(sum(score(values, remaining, 7) for remaining in range(20, 0, -1)))
