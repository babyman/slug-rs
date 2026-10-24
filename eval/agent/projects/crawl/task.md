# Crawl

<!-- DRAFT: only the sections written so far. The introduction, running and
testing, monsters, items, hunger, --More--, and the snapshot format follow
once the reference implements them; see outline.md. -->

## Random numbers

Every random choice in the game comes from one generator, and the order of
draws is part of the rules: each section below says exactly which draws
happen and in what order. Do not use `slug.random`; write this generator, so
that a snapshot can save and restore its state.

The state is one integer from 1 to 2^32 - 1. Seeding with `s` sets the state
to `s & 0xFFFFFFFF`, or to `0x9E3779B9` when that is 0.

- `next()` (xorshift32): with `x` the state,
  `x = x ^ ((x << 13) & 0xFFFFFFFF)`, then `x = x ^ (x >> 17)`, then
  `x = x ^ ((x << 5) & 0xFFFFFFFF)`; the state becomes `x`, and `x` is the
  result.
- `below(n)`, for `n` from 1 to 2^32: let `limit` be
  `2^32 - (2^32 % n)`. Draw `x = next()` until `x < limit`; the result is
  `x % n`. (This rejects the top of the range so every result is equally
  likely; it rarely draws more than once.)
- `roll(d, s)`: the sum of `d` results of `below(s) + 1`, drawn in turn.
  `roll(0, s)` is 0 and draws nothing.
- `pick(entries)`, for a list of entries with weights: one draw
  `r = below(total weight)`, then the first entry whose running total of
  weights, in list order, is greater than `r`.
- `shuffle(list)`: for `i` from `len - 1` down to 1, draw `j = below(i + 1)`
  and swap entries `i` and `j`.

Check values:

| after | results |
|---|---|
| seed 1 | `next()` three times: 270369, 67634689, 2647435461 |
| seed 1 | `below(6)` five times: 3, 1, 3, 5, 5 |
| seed 0 | `next()`: 1359758873 |
| seed 42 | `roll(3, 6)`: 10, and the state is then 476557059 |
| seed 7 | `shuffle([1, 2, 3, 4, 5])`: [1, 3, 2, 5, 4] |

## Data

`./data.slug` holds the font and the game's tables; import it with
`val data = import("data")` and do not change it (the tests supply their own
copy). Its tables, in the order the rules refer to them:

- `monsters`: the monster kinds, `Monster` structs with `letter`, `name`,
  `level`, `armour`, `damage` (`[dice, sides]`), `xp`, `depths` (the
  inclusive range of depths where the kind appears), and `flags`.
- `itemKinds`: potion, scroll, food, weapon, and armour, with weights.
- `weapons`, `armours`, `potions`, `scrolls`: the types of each kind, with
  weights; weapons have `damage` and armours an `armour` value.
- `colours` and `titles`: the unidentified names of potions and scrolls.

A table index is how the rules name a type: "potion type 2" is
`data.potions[2]`, confusion.

## A new game

A new game with seed `s` seeds the generator with `s`, then:

1. `shuffle(data.colours)`: potion type `n` is called by colour `n` of the
   result until identified.
2. `shuffle(data.titles)`: scroll type `n` gets title `n` of the result.
3. Generates level 1 (see **Levels**) and places the player there.

The player starts with 12 hit points of 12, strength 16 of 16, no gold,
experience level 1 with 0 experience points, and armour 0. The message line
shows `Welcome to the dungeon. Find the Orb on depth 8.`

## Levels

A level is a map of 80 columns by 22 rows, `x` from 0 to 79 and `y` from 0
to 21, shown on screen rows 1 to 22. Each square is one terrain character:

| character | terrain |
|---|---|
| space | rock |
| `.` | room floor |
| `#` | corridor |
| `+` | door |
| `-` | room wall, top and bottom rows, corners included |
| `\|` | room wall, left and right columns |
| `%` | stairs down |

Items, monsters, and the player stand on floor, corridor, door, or stairs
squares; they are not part of the terrain.

The map is divided into a 3 by 3 grid of **cells**, numbered 0 to 8 row by
row: cell `c` has its left column at `26 * (c % 3)` and its top row at
`7 * (c / 3)` (integer division), and is 26 columns wide and 7 rows high.
(Columns 78 and 79 and row 21 are never in a cell.) Two cells are
**adjacent** when they are side by side in the same row of the grid or one
above the other in the same column.

Generating the level at depth `d` (1 to 8) does the following steps in
order. A **free** square is a floor square (`.`) with no item and no
monster on it.

### 1. Gone cells

Draw `n = below(4)`. Then `n` times: draw `c = below(9)` until `c` is not
already gone, and mark cell `c` gone.

### 2. Rooms

For each cell `c` from 0 to 8, with `left` and `top` its corner:

- **A gone cell** has a single point instead of a room:
  `x = left + 1 + below(23)`, then `y = top + 1 + below(4)`. Nothing is
  drawn yet; the point becomes corridor when a corridor joins the cell.
- **Any other cell** has a room, a rectangle of walls around floor:
  1. `dark = below(10) < d - 1`.
  2. Width `w = 4 + below(22)`, then height `h = 4 + below(3)`, walls
     included.
  3. Its corner `x = left + below(26 - w)`, then `y = top + below(7 - h)`.
  4. Draw it: rows `y` and `y + h - 1` are `-` from column `x` to
     `x + w - 1`; each row between has `|` at columns `x` and `x + w - 1` and
     floor between.
  5. Gold: if `below(2) == 0`, the room has gold of amount
     `below(50 + 10 * d) + 2`, placed on a free square in the room (see
     below).
  6. A monster: if `below(100)` is less than 80 when the room has gold, or
     25 when it does not, place a monster on a free square in the room,
     then choose its kind and hit points. Its kind is `kinds[below(len)]`,
     where `kinds` lists, in table order, the indexes of the monsters whose
     `depths` include `d`. Its hit points are `roll(level, 8)` with `level`
     the kind's. It starts asleep.

A **free square in a room** is chosen by drawing `x = room.x + 1 +
below(w - 2)`, then `y = room.y + 1 + below(h - 2)`, repeating both draws
until the square is free.

### 3. Corridors

**Joining** two adjacent cells `a < b` carves a corridor between them:

- When `b = a + 1` (side by side), it leaves `a` from the right and enters
  `b` from the left. The start `S` is `a`'s point if `a` is gone, or else a
  door on `a`'s right wall: `S = (a.x + a.w - 1, a.y + 1 + below(a.h - 2))`.
  The end `E` is `b`'s point if `b` is gone, or else a door on `b`'s left
  wall: `E = (b.x, b.y + 1 + below(b.h - 2))`. Then the turn column
  `t = S.x + 1 + below(E.x - S.x - 1)`. The corridor runs from `S` right
  along row `S.y` to column `t`, then up or down column `t` to row `E.y`,
  then right along row `E.y` to `E`.
- When `b = a + 3` (one above the other), the same with rows and columns
  exchanged: `S` is `a`'s point or a door on `a`'s bottom wall,
  `S = (a.x + 1 + below(a.w - 2), a.y + a.h - 1)`; `E` is `b`'s point or a
  door on `b`'s top wall, `E = (b.x + 1 + below(b.w - 2), b.y)`; the turn
  row is `t = S.y + 1 + below(E.y - S.y - 1)`; and the corridor runs down
  column `S.x` to row `t`, across row `t` to column `E.x`, then down to `E`.

`S` and `E` become doors, or corridor for a gone cell's point. Every rock
square on the path between them becomes corridor; squares that are already
corridor stay so. (The path never crosses a room.)

The corridors:

1. **A spanning tree.** Draw `r = below(9)`; the graph holds just `r`.
   Then, until the graph holds all nine cells: among the cells adjacent to
   `r` that are not in the graph, choose one by **reservoir** (below). If
   there is none, draw `r = below(9)` until `r` is in the graph. Otherwise
   join `r` to the chosen cell and add it to the graph; `r` stays the same.
2. **Extra corridors.** Draw `n = below(5)`. Then `n` times: draw
   `r = below(9)`; among the cells adjacent to `r` that `r` is not already
   joined to, choose one by reservoir, and if there is one, join `r` to it.

A **reservoir** choice goes through the cells 0 to 8 in order, skipping
those that do not qualify; at the `j`-th one that does (counting from 1) it
draws `below(j)`, and that cell becomes the choice if the draw is 0. So with
three qualifying cells it draws `below(1)`, `below(2)`, `below(3)`, and the
last cell whose draw was 0 is chosen.

### 4. Items

Nine times: if `below(100) < 36`, make a new item, then place it on a free
square anywhere (below).

A **new item**: its kind is `pick(data.itemKinds)`, then:

- potion: type `pick(data.potions)`
- scroll: type `pick(data.scrolls)`
- food: no more draws
- weapon: type `pick(data.weapons)`, then a bonus
- armour: type `pick(data.armours)`, then a bonus

A **bonus** is one draw `b = below(100)`: -1 if `b < 10`, +1 if `b < 25`,
and 0 otherwise.

At depth 8, the Orb is then placed on a free square anywhere.

A **free square anywhere** is chosen by drawing `c = below(9)` until cell `c`
is not gone, then `x` and `y` in its room as for a free square in a room;
if that square is not free, start again by drawing `c`.

### 5. Stairs and the player

The stairs replace the floor of a free square anywhere. Then the player is
placed on a free square anywhere.

### Example

Seed 1, after the two shuffles, generates this level 1 (items, monsters,
and the player not shown):

```
                                                           --------------
  -----------------     ##################                 |............|
  |...............+######            #   #                 |............|
  |...............|                 ##   ##################+............|
  -----------------                 #                      |............|
                                    #                      --------------
                                    #
 ------------------------         --+---                         --------
 |......................|         |....+#########################+......|
 |......................+#        |....|                         |.....%|
 ------------+-----------#########+....|                         --------
             #                    --+---
        ######                      ########
        #                                  #
        #                                  #             ---------------
--------+----------------                  #             |.............|
|.......................|    --------------+-------     #+.............|
|.......................+####+....................|     #|.............|
|.......................|    |....................+######---------------
-------------------------    ----------------------
```

Cell 1 (top middle) is gone; its point is (37, 1), on the corridor along
row 1. Trailing spaces are not shown; every map row is 80 characters.

## The screen

The display is 640 by 240 pixels: 24 rows of 80 characters, each character
an 8 by 10 pixel cell. Character `c` (32 to 126) of `data.font` is drawn in
the cell at column `col` and row `row` as 10 bytes: byte `r` of the glyph is
byte `col` of frame row `10 * row + r`. Open the display and colour the top
and bottom rows like this, and present the frame every frame:

```slug
arcade.open("Crawl", 640, 240, {scale: 2})
arcade.overlay(0, 0, 640, 10, 0xffff60)
arcade.overlay(0, 230, 640, 10, 0x60ff60)
```

The 24 rows of text:

- Row 0, the **message line**: the messages of the last command, joined
  with two spaces, or blank.
- Rows 1 to 22: the map, map row `y` on screen row `y + 1`. Each square
  shows, in order of precedence: `@` for the player; a monster's letter if
  the player can see the square (see **Seeing**); otherwise what the player
  remembers there.
- Row 23, the **status line**:
  `Depth:1  Gold:0  Hp:12(12)  Str:16(16)  Arm:0  Exp:1/0`, that is the
  depth, gold, hit points (maximum in parentheses), strength (maximum in
  parentheses), armour, and experience level `/` experience points, with two
  spaces between fields and no padding.

Rows end with spaces where their text is shorter than 80 characters.

## Commands

The game waits for commands. In each frame, the command is the first key
in this list that was pressed in that frame (`arcade.pressed`); other keys
pressed in the same frame are ignored, and a frame with none of them does
nothing:

`h`, `j`, `k`, `l`, `y`, `u`, `b`, `n`, `left`, `down`, `up`, `right`,
`space`, `enter`

| key | command |
|---|---|
| `h` or `left` | move west (x - 1) |
| `j` or `down` | move south (y + 1) |
| `k` or `up` | move north (y - 1) |
| `l` or `right` | move east (x + 1) |
| `y`, `u`, `b`, `n` | move north-west, north-east, south-west, south-east |
| a move key with `shift` held (`arcade.down`) | run in that direction |
| `space` | rest one turn |
| `enter` | take the stairs down |

Every command first clears the message line; the messages it produces are
then shown until the next command. A command that takes game time ends one
or more **turns**: each turn adds 1 to the turn count and then updates what
the player sees and remembers. A command that does nothing takes no turn.

### Moving

The player moves one square in the direction, taking a turn, when the
target square is floor, corridor, door, or stairs, and no monster is on it.
A diagonal move also needs both squares that share a side with the
player's square and the target to be floor, corridor, door, or stairs; so
the player cannot move diagonally into or out of a doorway, or cut a corner
of rock. A move that cannot be made does nothing and takes no turn.

Standing on an item does nothing yet.

### Running

Running repeats moves in one direction: move as above; if the move could
not be made, the run ends. After each move that is made (each its own
turn), the run ends if the player now stands on a door, the stairs, or an
item, or can see any monster. Otherwise the run continues with the next
move. A run whose first move cannot be made takes no turn.

### Resting

`space` takes one turn without moving.

### Stairs

`enter` on the stairs takes the player down: the level at the next depth is
generated (see **Levels**), the player is placed on it, and the command
takes one turn. On depth 8 the stairs lead out only with the Orb; without
it the message is `You need the Orb to leave.` and no turn passes. Off the
stairs, the message is `I see no way down.` and no turn passes.

## Seeing

The player **can see** a square when it is the player's own square or one
of its eight neighbours, or when the player is in a lit room and the square
is in that room's rectangle, walls included. The player is **in** a room
when the room's rectangle, walls included, holds the player's square, so
standing in a doorway counts as being in the room. Gone cells' points are
not rooms. In a dark room, or in a corridor, the player sees only the
neighbours.

The player **remembers** what they last saw on each square: when the game
starts on a level, and at the end of every turn, each square the player
can see is remembered as the glyph of the item on it, if any, or else its
terrain. Squares never seen are remembered as spaces. A new level starts
with nothing remembered.

| glyph | item |
|---|---|
| `*` | gold |
| `!` | potion |
| `?` | scroll |
| `:` | food |
| `)` | weapon |
| `]` | armour |
| `,` | the Orb |

Monsters are not remembered: a monster's letter shows only while the
player can see its square.
