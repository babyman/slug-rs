def advance(remaining, position, velocity):
    while remaining:
        remaining -= 1
        position += velocity
        velocity += 0.000_001
    return position


position = advance(50_000, 0.0, 0.01)
print("ok" if position > 1_000.0 else "unexpected")
