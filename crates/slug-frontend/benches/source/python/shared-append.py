def build(remaining):
    acc, previous, total = [], [], 0
    while remaining:
        acc, previous, total = acc + [remaining], acc, total + len(previous)
        remaining -= 1
    return total + len(acc) + len(previous)


print(build(8_000))
