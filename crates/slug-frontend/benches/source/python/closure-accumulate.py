from functools import reduce


def collect(count):
    readings = []
    total = 0
    for remaining in range(count, 0, -1):
        reading = remaining * 7 % 101
        readings.append(reading)
        kept = list(filter(lambda value: value > reading / 2, [reading, reading + 1, reading + 2]))
        total += reduce(lambda acc, value: acc + value, kept, 0)
    return total + len(readings)


print(collect(20_000))
