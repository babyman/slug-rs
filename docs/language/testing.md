# Testing a Program

Test a program's logic in Slug, in one process: put the logic in a module,
import that module from a test program, and run many assertions in one
`slug` run. Each test then costs a function call rather than a process, and
the first failure comes with slug's full diagnostic.

## Keep the logic in a module

The entry program handles only input and output: options, files, the
display. Everything a test wants to check lives in a module beside it, with
the state it works on passed in or kept there, and exported:

```slug
// robot.slug: the program's logic. main.slug, the entry program, imports it.
val { reduce } = import("slug.std")

export val newGame = fn():map { {x: 0, y: 0, moves: 0, log: []} }

// A game restored from a snapshot is just the snapshot's map.
export val load = fn(snapshot:map):map { snapshot }

export val command = fn(game:map, key:str):map {
    val step = match key {
        "l" => [1, 0]
        "h" => [-1, 0]
        "j" => [0, 1]
        "k" => [0, -1]
        _ => nil
    }
    if (step == nil) {
        game
    } else {
        val x = game.x + step[0]
        val y = game.y + step[1]
        if (x < 0 || x > 4 || y < 0 || y > 4) {
            game + {log: ["Bump!"]}
        } else {
            game + {x: x, y: y, moves: game.moves + 1, log: []}
        }
    }
}

export val commands = fn(game:map, keys:list):map {
    reduce(keys, game, fn(g, key) { command(g, key) })
}
```

## Write the tests as a program

`slug.test` (see `slug-docs/library/core/test.slug`) has `assertEqual`,
`assertTrue`, `assertThrows`, and the rest; a failed assertion throws an
error holding the expected and actual values. A test program imports the
module, builds the states it needs (a new game, or a snapshot parsed from
JSON, as a saved game would be read), and runs each test in turn:

```slug
// tests.slug: run with `slug tests.slug`.
val test = import("slug.test")
val json = import("slug.fmt.json")
val robot = import("robot")

val corner = robot.load(json.parse("{\"x\": 4, \"y\": 4, \"moves\": 8, \"log\": []}"))

val tests = [
    ["a new game starts at the origin", fn() {
        val game = robot.newGame()
        test.assertEqual([game.x, game.y, game.moves], [0, 0, 0])
    }],
    ["moves change the position and count", fn() {
        val game = robot.commands(robot.newGame(), ["l", "l", "j"])
        test.assertEqual([game.x, game.y, game.moves], [2, 1, 3])
    }],
    ["walls stop the robot", fn() {
        val game = robot.command(corner, "l")
        test.assertEqual(game.x, 4)
        test.assertEqual(game.moves, 8, "a bump is not a move")
        test.assertEqual(game.log, ["Bump!"])
    }],
]

// Names each test before running it. The first failure is not caught, so
// slug reports it in full and exits with status 1.
val main = fn() {
    val run = fn(i:num) {
        if (i < len(tests)) {
            val name = tests[i][0]
            println("test: $name")
            tests[i][1]()
            recur(i + 1)
        }
    }
    run(0)
    val count = len(tests)
    println("all $count tests passed")
}
```

A failed assertion reports its message and values, and the test's line:

```
test: walls stop the robot
slug: runtime error: uncaught throw: struct {"type": "test.failure", "msg": "a bump is not a move", "code": nil, "data": {"expected": 9, "actual": 8}, "cause": nil}
  ...
  in <fn #1> at tests.slug:20:13
```

and an error in the module reports the line that failed and the calls that
led to it, from the test down. Fix it and run `slug tests.slug` again; add a
test for each rule as you write it.

## Checking the whole program

Test the entry program itself with a few end-to-end runs: run it as the
user would, then read what it wrote. When a script in another language runs
`slug` for you, have it print the program's stdout and stderr whenever the
run fails, since slug's diagnostic is on stderr; otherwise a failure shows
only an exit status.

Installed clutches can also tag functions with `@test` and `@testWith` and
run them with `slug test`; see `slug-docs/library/core/test.slug`.
