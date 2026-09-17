def increment(value):
    return value + 1


def run(remaining, value):
    while remaining:
        remaining -= 1
        value = increment(value)
    return value


print(run(100_000, 0))
