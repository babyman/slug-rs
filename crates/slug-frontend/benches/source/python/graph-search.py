from collections import deque

NODES = 3_000


def neighbours(node):
    return [(node * 7 + 1) % NODES, (node * 13 + 5) % NODES, (node + 1) % NODES]


def main():
    graph = {node: neighbours(node) for node in range(NODES)}
    seen = {0: 0}
    queue = deque([0])
    while queue:
        node = queue.popleft()
        distance = seen[node] + 1
        for following in graph[node]:
            if following not in seen:
                seen[following] = distance
                queue.append(following)

    total = sum(seen[node] for node in range(NODES))
    farthest = max(seen[node] for node in range(NODES))
    print(len(seen), total, farthest)


main()
