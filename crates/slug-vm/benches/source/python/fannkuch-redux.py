def flip(values, count):
    return list(reversed(values[:count])) + values[count:]


def flip_count(values):
    total = 0
    while values[0] != 1:
        values = flip(values, values[0])
        total += 1
    return total


def generate(choices, prefix, checksum, maximum, sign):
    if not choices:
        flips = flip_count(prefix)
        return checksum + sign * flips, max(maximum, flips), -sign

    for _ in range(len(choices)):
        head, *tail = choices
        checksum, maximum, sign = generate(tail, prefix + [head], checksum, maximum, sign)
        choices = tail + [head]
    return checksum, maximum, sign


checksum, maximum, _ = generate(list(range(1, 6)), [], 0, 0, 1)
print(checksum, maximum)
