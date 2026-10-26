def line(index):
    label = "even" if index % 2 == 0 else "odd"
    return f"item {index} is {label}; "


def main():
    text, lines = "", 0
    for remaining in range(20_000, 0, -1):
        text += line(remaining)
        lines += 1
    print(len(text), lines, text[0:12])


main()
