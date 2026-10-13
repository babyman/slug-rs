from dataclasses import dataclass


@dataclass
class Num:
    value: object


@dataclass
class Var:
    name: str


@dataclass
class Binary:
    op: str
    left: object
    right: object


DIGITS = {str(digit): digit for digit in range(10)}
LETTERS = set("abcxy")
OPERATORS = set("+-*/()")


def tokenize(source):
    tokens, index = [], 0
    while index < len(source):
        ch = source[index]
        if ch == " ":
            index += 1
        elif ch in DIGITS:
            value = 0
            while index < len(source) and source[index] in DIGITS:
                value = value * 10 + DIGITS[source[index]]
                index += 1
            tokens.append(("num", value))
        elif ch in LETTERS:
            tokens.append(("var", ch))
            index += 1
        elif ch in OPERATORS:
            tokens.append(("op", ch))
            index += 1
        else:
            raise ValueError(ch)
    return tokens


def peek(tokens, position):
    return tokens[position] if position < len(tokens) else ("end", None)


def parse_primary(tokens, position):
    kind, value = peek(tokens, position)
    if kind == "num":
        return Num(value), position + 1
    if kind == "var":
        return Var(value), position + 1
    if value != "(":
        raise ValueError(value)
    inner, position = parse_sum(tokens, position + 1)
    return inner, position + 1


def parse_product(tokens, position):
    left, position = parse_primary(tokens, position)
    while True:
        kind, value = peek(tokens, position)
        if kind != "op" or value not in ("*", "/"):
            return left, position
        right, position = parse_primary(tokens, position + 1)
        left = Binary(value, left, right)


def parse_sum(tokens, position):
    left, position = parse_product(tokens, position)
    while True:
        kind, value = peek(tokens, position)
        if kind != "op" or value not in ("+", "-"):
            return left, position
        right, position = parse_product(tokens, position + 1)
        left = Binary(value, left, right)


def apply(op, left, right):
    if op == "+":
        return left + right
    if op == "-":
        return left - right
    if op == "*":
        return left * right
    return left / right


def simplify(node):
    if not isinstance(node, Binary):
        return node
    left, right = simplify(node.left), simplify(node.right)
    if isinstance(left, Num) and isinstance(right, Num):
        return Num(apply(node.op, left.value, right.value))
    return Binary(node.op, left, right)


def evaluate(node, env):
    if isinstance(node, Num):
        return node.value
    if isinstance(node, Var):
        return env[node.name]
    return apply(node.op, evaluate(node.left, env), evaluate(node.right, env))


def parse(source):
    tree, _ = parse_sum(tokenize(source), 0)
    return simplify(tree)


SOURCES = [
    "(a + b * 3) - (c / 2 + a * (b - 1))",
    "x * x + 2 * x * y + y * y - (4 * 5 + 6) / 2",
    "((a + 1) * (b + 2) * (c + 3)) / (1 + 2 * 3) + x - y",
]

programs = [parse(source) for source in SOURCES]
parsed = sum(len([parse(source) for source in SOURCES]) for _ in range(300))
total = 0
for remaining in range(5_000, 0, -1):
    env = {
        "a": remaining % 7,
        "b": remaining % 11,
        "c": remaining % 13,
        "x": remaining % 17,
        "y": remaining % 19,
    }
    subtotal = 0
    for program in programs:
        subtotal += evaluate(program, env)
    total += subtotal
print(parsed, int(total * 1_000))
