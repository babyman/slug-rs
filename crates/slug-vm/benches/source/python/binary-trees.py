def build(depth):
    if depth == 0:
        return 1
    return (build(depth - 1), build(depth - 1))


def check(tree):
    if tree == 1:
        return 1
    return 1 + check(tree[0]) + check(tree[1])


print(check(build(15)))
