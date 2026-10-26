def word(index):
    return "w" + str((index * 7_919) % 509)


def main():
    counts = {}
    for remaining in range(50_000, 0, -1):
        key = word(remaining)
        counts[key] = counts.get(key, 0) + 1

    total, largest = 0, 0
    for key in list(counts):
        value = counts[key]
        total += value
        largest = max(largest, value)
    print(len(counts), total, largest)


main()
