VOCABULARY = ["slug", "virtual", "machine", "bytecode", "a", "map", "of", "closures", "the",
              "frame", "dispatch", "loop", "persistent", "value"]
VOWELS = set("aeiou")
WIDTH = 60


def main():
    text, line, lines, vowel_count = "", "", 0, 0
    for remaining in range(30_000, 0, -1):
        word = VOCABULARY[(remaining * 7) % len(VOCABULARY)]
        for index in range(len(word)):
            if word[index] in VOWELS:
                vowel_count += 1
        if not line:
            line = word
        elif len(line) + 1 + len(word) > WIDTH:
            text += line + "\n"
            lines += 1
            line = word
        else:
            line = f"{line} {word}"
    text += line + "\n"
    lines += 1
    print(len(text), lines, vowel_count)


main()
