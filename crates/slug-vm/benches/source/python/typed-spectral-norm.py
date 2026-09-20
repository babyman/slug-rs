def column_sum(row, column, remaining, total):
    while remaining:
        total += 1.0 / (row + column + 1.0)
        column += 1
        remaining -= 1
    return total


def rows(row, remaining, total):
    while remaining:
        total += column_sum(row, 0, 100, 0.0)
        row += 1
        remaining -= 1
    return total


norm = rows(0, 100, 0.0)
print("ok" if norm > 100.0 else "unexpected")
