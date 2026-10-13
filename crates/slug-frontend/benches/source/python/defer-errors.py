class Failure(Exception):
    def __init__(self, code):
        self.code = code


def step(value):
    try:
        if value % 8 == 0:
            raise Failure(1)
        return value % 5
    except Failure as err:
        return err.code


def tracked(value, cleanups):
    done = cleanups
    try:
        return step(value)
    finally:
        done += 1


print(sum(tracked(remaining, 0) for remaining in range(100_000, 0, -1)))
