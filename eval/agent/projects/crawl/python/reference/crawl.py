"""The Python arm's interface (see python/task.md) over the reference Game."""
from game import Game


def new_game(seed):
    return Game(seed)


def load(snapshot):
    return Game.load(snapshot)


def press(game, key, shift=False):
    game.command(key, shift)


def screen(game):
    return [row.ljust(80) for row in game.text()]


def snapshot(game):
    return game.state()
