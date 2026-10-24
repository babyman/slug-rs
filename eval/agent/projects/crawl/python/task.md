# Crawl

Write `./crawl.py`, a Python module that plays Crawl, a small Rogue-like
game: the player explores eight levels of rooms and corridors, fights
monsters, collects items, and escapes with the Orb from the deepest level.
Everything below is exact: the hidden tests play your module with scripted
keys and compare every screen it shows, and the game's final state, with a
reference implementation's. Random choices come from a generator this
specification defines, so the same seed and keys always give the same game.

You may split the program into modules beside `crawl.py`. `./data.py` holds
the game's tables (see **Data**); do not change it (the tests supply their
own copy). Use only Python's standard library.

## Running and testing

`crawl.py` defines these functions:

| function | does |
|---|---|
| `new_game(seed)` | returns a new game with that seed (see **A new game**) |
| `load(snapshot)` | returns the game saved in a snapshot (see **Snapshots**), a `dict` as `json.load` reads it, exactly as it was: nothing happens on loading |
| `press(game, key, shift=False)` | gives the game one key press (see **Commands**); `shift` tells whether shift is held |
| `screen(game)` | returns the screen as it is now: a list of 24 strings (see **The screen**) |
| `snapshot(game)` | returns the game's snapshot, a `dict` that `json.dumps` can write |

A game may be any object you like. `key` is one of `"a"` to `"z"`,
`"left"`, `"down"`, `"up"`, `"right"`, `"space"`, and `"enter"`.

The tests start a game with `new_game` or `load`, then press keys one at a
time, and compare `screen(game)` before the first key and after every key,
and `snapshot(game)` after the last. Each test may take up to 20 seconds
plus one second for every 75 keys, and they press up to 1500 keys, so the
game must take well under 10 milliseconds per key.

## Snapshots

A snapshot is one JSON object holding the whole state of a game; `load`
reads one and `snapshot` returns one. The tests compare your snapshot with
the reference's as JSON values: key order and spacing do not matter,
but every key below must be present with exactly the right value, and lists
in the order given. Numbers are integers, and JSON's `null` is `None`.

| key | value |
|---|---|
| `seed` | the game's seed |
| `rng` | the generator's state (see **Random numbers**) |
| `turn` | the turn count |
| `depth` | the current depth, 1 to 8 |
| `colours` | the game's shuffled `data.COLOURS`, 15 strings |
| `titles` | the game's shuffled `data.TITLES`, 12 strings |
| `known` | `{"potions": [...], "scrolls": [...]}`: the identified types, each list in the order they were identified |
| `messages` | the messages showing on the message line, as a list of strings |
| `more` | the messages waiting behind `--More--`, as a list |
| `prompt` | the key of the item command whose prompt is showing (`"e"`, `"q"`, `"r"`, `"w"`, `"a"`, or `"d"`), or `null` |
| `showingPack` | whether the pack is shown (`i`) |
| `dead`, `won` | whether the player has died, or won |
| `sense` | whether the player is sensing monsters |
| `map` | the level's terrain: 22 strings of 80 characters |
| `memory` | what the player remembers: 22 strings of 80 characters |
| `rooms` | the nine cells' rooms in cell order: `{"x", "y", "w", "h", "gone", "dark"}`; a gone cell's point has `w` and `h` 1 and `dark` false |
| `items` | the items on the level, in the order they were made or dropped: `{"x", "y", "item"}` |
| `monsters` | the monsters, in list order: `{"x", "y", "kind", "hp", "maxHp", "state"}`, with `state` `"asleep"` or `"awake"` |
| `pack` | the pack's entries in letter order: `{"letter", "item", "count"}` |
| `player` | `{"x", "y", "hp", "maxHp", "str", "maxStr", "gold", "level", "xp", "weapon", "armour", "food", "regen", "frozen", "confused", "blind"}`: `weapon` and `armour` are the letters of the wielded weapon and worn armour, or `null`; `food` is the food counter, `regen` the healing counter, and `frozen`, `confused`, and `blind` the turns left of each |

An item is one of `{"kind": "gold", "amount": n}`, `{"kind": "food"}`,
`{"kind": "weapon", "type": t, "bonus": b}`, `{"kind": "armour", "type": t,
"bonus": b}`, `{"kind": "potion", "type": t}`, `{"kind": "scroll", "type":
t}`, or `{"kind": "orb"}`, with `t` an index into the kind's table.

Every rule below is stated in terms of this state. For example, a new game
with seed 1 has `"turn": 0`, `"depth": 1`, `"messages": ["Welcome to the
dungeon. Find the Orb on depth 8."]`, `"more": []`, `"prompt": null`, and
the starting pack.

## Random numbers

Every random choice in the game comes from one generator, and the order of
draws is part of the rules: each section below says exactly which draws
happen and in what order. Do not use Python's `random`; write this generator, so
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

`./data.py` holds the game's tables; import it with `import data` and do
not change it (the tests supply their own copy). Its tables, in the order
the rules refer to them, are lists:

- `MONSTERS`: the monster kinds, dicts with `letter`, `name`, `level`,
  `armour`, `damage` (`[dice, sides]`), `xp`, `depths` (the inclusive range
  of depths where the kind appears), and `flags`.
- `ITEM_KINDS`: potion, scroll, food, weapon, and armour, with weights.
- `WEAPONS`, `ARMOURS`, `POTIONS`, `SCROLLS`: the types of each kind, with
  weights; weapons have `damage` and armours an `armour` value.
- `COLOURS` and `TITLES`: the unidentified names of potions and scrolls.

A table index is how the rules name a type: "potion type 2" is
`data.POTIONS[2]`, confusion.

## A new game

A new game with seed `s` seeds the generator with `s`, then:

1. `shuffle(data.COLOURS)`: potion type `n` is called by colour `n` of the
   result until identified.
2. `shuffle(data.TITLES)`: scroll type `n` gets title `n` of the result.
3. Generates level 1 (see **Levels**) and places the player there.

The player starts with 12 hit points of 12, strength 16 of 16, no gold,
experience level 1 with 0 experience points, a food counter of 1300 (see
**Hunger**), and this pack (see **Items**):

```
a) a +1 mace (weapon in hand)
b) +1 ring mail (being worn)
c) a food ration
```

The message line shows `Welcome to the dungeon. Find the Orb on depth 8.`

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

A **new item**: its kind is `pick(data.ITEM_KINDS)`, then:

- potion: type `pick(data.POTIONS)`
- scroll: type `pick(data.SCROLLS)`
- food: no more draws
- weapon: type `pick(data.WEAPONS)`, then a bonus
- armour: type `pick(data.ARMOURS)`, then a bonus

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

The screen is 24 rows of 80 characters, which `screen(game)` returns as a
list of 24 strings, row 0 first. The rows:

- Row 0, the **message line**: the messages of the last command, joined
  with two spaces, or blank; see **Messages**.
- Rows 1 to 22: the map, map row `y` on screen row `y + 1`. Each square
  shows, in order of precedence: `@` for the player; a monster's letter if
  the player can see the square (see **Seeing**) or is sensing monsters
  (see **Potions**); otherwise what the player remembers there. While the
  pack is shown (the `i` command), these rows instead show the pack's
  inventory lines (see **Items**), one per row from row 1, in letter order,
  and blank rows after them.
- Row 23, the **status line**:
  `Depth:1  Gold:0  Hp:12(12)  Str:16(16)  Arm:4  Exp:1/0`, that is the
  depth, gold, hit points (maximum in parentheses), strength (maximum in
  parentheses), armour (see **Combat**), and experience level `/`
  experience points, with two spaces between fields and no padding. When
  the player is hungry, two spaces and `Hungry`, `Weak`, or `Faint` follow
  (see **Hunger**).

Rows end with spaces where their text is shorter than 80 characters.

## Commands

The game waits for commands: each `press` gives it one key. What a key does depends on what the game is waiting for, checked in this
order:

1. **`--More--` is showing:** only `space` does anything (see **Messages**).
2. **The game is over** (see **The end**): only `enter` does anything.
3. **The pack is shown:** only `space` does anything: it hides the pack
   and clears the message line.
4. **An item prompt is showing:** a letter key answers it (see **Item
   commands**); `space` cancels it, clearing the message line with no turn
   passing; other keys are ignored.
5. **Otherwise**, these keys are commands; other keys are ignored:

| key | command |
|---|---|
| `h` or `left` | move west (x - 1) |
| `j` or `down` | move south (y + 1) |
| `k` or `up` | move north (y - 1) |
| `l` or `right` | move east (x + 1) |
| `y`, `u`, `b`, `n` | move north-west, north-east, south-west, south-east |
| a move key with `shift` held | run in that direction |
| `space` | rest one turn |
| `enter` | take the stairs down |
| `i` | show the pack |
| `e`, `q`, `r`, `w`, `a`, `d` | eat, quaff, read, wield, wear or take off, drop (see **Item commands**) |

Every command (and every answer to an item prompt) first clears the message
line; the messages it produces are then shown until the next command (see
**Messages**). A command that takes game time ends one or more **turns**.
At the end of each turn, after the player's action:

1. the monsters act, in list order (see **Monsters**), stopping if the
   player dies;
2. if the player is alive, the player's upkeep happens (see **Hunger**,
   **Healing**, and **Potions**);
3. the turn count goes up by 1;
4. the player looks around (see **Seeing**).

After a command (or prompt answer) has done its action, if the player is
**frozen** for `n` turns (see **Combat** and **Hunger**), then while `n`
is above 0 and the player is alive: `n` goes down by 1, and a turn passes as
if the player rested (so during the last frozen turn the player is no
longer frozen). The player stays frozen through turns that freeze them
further. Their messages belong to the same command. A command that does
nothing takes no turn.

### Moving

A **step** from one square to a neighbour is allowed when the target
square is floor, corridor, door, or stairs, and, for a diagonal step, both
squares that share a side with the start and the target are too; so no one
moves diagonally into or out of a doorway, or cuts a corner of rock. This
rule is the same for the player and for monsters.

When the step in the command's direction is allowed, the player moves
there and picks up what lies there (see **Items**), taking a turn; if a
monster is on that square, the player attacks it instead (see **Combat**),
which also takes a turn. When the step is not allowed, nothing happens and
no turn passes.

While the player is **confused** (see **Potions**), every move or run
command draws `d = below(8)` first and makes a single move in direction `d`
of the list in **Monsters**, whichever key was pressed; it is never a run.

### Running

Running repeats moves in one direction. Before each move, if a monster is
on the target square or the step is not allowed, the run ends; running
never attacks. After each move (each its own turn), the run ends if the
player now stands on a door, the stairs, or an item, can see any monster,
is dead, or the command has produced a message (picking something up does).
Otherwise the run continues with the next move. A run whose first move
cannot be made takes no turn.

### Resting

`space` takes one turn without moving.

### Stairs

`enter` on the stairs takes the player down: the level at the next depth is
generated (see **Levels**) and the player is placed on it. The turn count
goes up by 1, but the new level's monsters do not act until the next turn,
and there is no upkeep. On depth 8 the stairs lead out of the dungeon: with
the Orb in the pack the player wins (see **The end**); without it the
message is `You need the Orb to leave.` and no turn passes. Off the stairs,
the message is `I see no way down.` and no turn passes.

## Items

An item on the floor or in the pack is one of these, as in **Levels**:
gold with an amount; food; a weapon or armour with a type and a bonus; a
potion or scroll with a type; or the Orb.

### Names

Messages and the inventory name `n` of an item like this (`n` is 1 except
for stacks):

| item | one | more |
|---|---|---|
| food | `a food ration` | `3 food rations` |
| weapon | `a +1 mace`, `a -1 dagger`, `a +0 long sword` | |
| armour | `+1 ring mail`, `-1 plate mail`, `+0 leather armour` | |
| potion, identified | `a potion of healing` | `2 potions of healing` |
| potion, not identified | `an amber potion`, `a pink potion` | `2 amber potions` |
| scroll, identified | `a scroll of identify` | `2 scrolls of identify` |
| scroll, not identified | `a scroll titled "zorn plib"` | `2 scrolls titled "zorn plib"` |
| the Orb | `the Orb` | |

A weapon's or armour's bonus is always shown with its sign. An
unidentified potion is named by its colour from the game's shuffled
`colours` (potion type `t` has colour `t`), with `an` before a colour that
starts with a vowel (`a`, `e`, `i`, `o`, `u`) and `a` otherwise; a scroll by
its title from the shuffled `titles`. A potion or scroll type is
**identified** once the player has quaffed or read one, or a scroll of
identify has named it; identification lasts for the rest of the game.

### The pack

The pack holds at most 22 entries. Each has a letter, an item, and a count.
Food, and potions or scrolls of the same type, **stack**: an item that
stacks with an entry adds 1 to its count. Any other item gets a new entry
with count 1 and the lowest letter from `a` to `z` that no entry has; the
pack is kept in letter order. At most one weapon is **wielded** and at most
one armour is **worn**; they are remembered by letter.

An entry's **inventory line** is its letter, `) `, and the name of its
count of its item, then ` (weapon in hand)` if it is the wielded weapon or
` (being worn)` if it is the worn armour: `a) a +1 mace (weapon in hand)`.

When an entry's last item leaves the pack, the entry goes, and if it was
wielded or worn, it no longer is.

### Picking up

When the player moves onto a square with an item:

- gold is added to the player's gold and leaves the floor, with the message
  `You found 21 gold pieces.` (the amount);
- any other item goes into the pack and leaves the floor, with the message
  of its entry's inventory line (after stacking: `c) 2 food rations`); but
  if it does not stack and the pack has 22 entries, it stays on the floor
  and the message is `Your pack is full.`

### Item commands

`e`, `q`, `r`, `w`, `a`, and `d` each act on one pack entry, which the
player names by its letter at a prompt:

| key | prompt verb | kind | when there is none | wrong kind |
|---|---|---|---|---|
| `e` | `Eat` | food | `You have nothing to eat.` | `You can't eat that.` |
| `q` | `Quaff` | potion | `You have nothing to quaff.` | `You can't quaff that.` |
| `r` | `Read` | scroll | `You have nothing to read.` | `You can't read that.` |
| `w` | `Wield` | weapon | `You have nothing to wield.` | `You can't wield that.` |
| `a` | `Wear or take off` | armour | `You have nothing to wear.` | `You can't wear that.` |
| `d` | `Drop` | any | `You have nothing to drop.` | |

The command checks first: for `r` while the player is blind, the message
is `You can't see to read.`; if no pack entry has the kind (any entry, for
`d`), the message is the "when there is none" one. In both cases nothing
more happens and no turn passes. Otherwise the message line shows the
prompt `Quaff which item? (space to cancel)` (with the verb) and the game
waits for an answer.

A letter key answers the prompt. The message line is cleared, and:

- if no entry has that letter: `You have no item x.` (the letter);
- if the entry is not the command's kind: the "wrong kind" message;

and no turn passes. Otherwise the command acts on that entry:

- **Eat:** one food leaves the pack. The food counter becomes
  `max(counter, 0) + 1100 + below(400)`, at most 2000. Message:
  `Yum, that tasted good.` One turn.
- **Quaff:** one potion leaves the pack, its type is identified, and it
  takes effect (see **Potions**). One turn.
- **Read:** one scroll leaves the pack, its type is identified, and it
  takes effect (see **Scrolls**). One turn.
- **Wield:** if it is already wielded: `You are already wielding that.`, no
  turn. Otherwise it becomes the wielded weapon: `You are now wielding a +1
  mace.` One turn.
- **Wear or take off:** if it is the worn armour, it is taken off:
  `You took off +1 ring mail.`, one turn. Otherwise, if other armour is
  worn: `You are already wearing armour.`, no turn. Otherwise it becomes
  the worn armour: `You are now wearing +1 ring mail.`, one turn.
- **Drop:** if it is the worn armour: `You can't drop armour you are
  wearing.`, no turn. If an item already lies on the player's square, or
  the square is the stairs: `There is no room to drop that here.`, no
  turn. Otherwise one item of the entry leaves the pack and is added to
  the end of the level's item list on the player's square:
  `Dropped a +1 mace.` (the name of one). One turn.

`i` shows the pack: the message line shows `Press space to continue.` and
rows 1 to 22 the inventory lines, until `space`. It takes no turn. With an
empty pack the message is `You are empty-handed.` instead and the pack is
not shown.

### Potions

| type | effect | message |
|---|---|---|
| healing | hit points + `roll(level, 4)` (the player's experience level); if that is above the maximum, the maximum goes up by 1 and the hit points become the new maximum | `You begin to feel better.` |
| poison | strength - (`below(3) + 1`), not below 3 | `You feel very sick now.` |
| confusion | confused for `below(8) + 8` more turns | `Wait, what's going on here?` |
| blindness | blind for `below(50) + 50` more turns | `A cloak of darkness falls around you.` |
| gain strength | strength + 1, and the maximum strength rises to match if it is lower | `You feel stronger.` |
| see monsters | if the level has monsters, the player senses them until leaving the level: every monster's letter shows on the map wherever it is | `You sense the presence of monsters.`, or with no monsters `You have a strange feeling for a moment.` |

Confusion and blindness are counters of turns. In the upkeep at the end of
each turn (after hunger and healing), each that is above 0 goes down by 1;
when confusion reaches 0 the message is `You feel less confused now.`, and
when blindness does, `The darkness lifts.`

While **blind**, the player can see no square at all (see **Seeing**):
nothing is remembered, no monster shows unless sensed, the floating eye
cannot freeze the player, and scrolls cannot be read.

### Scrolls

| type | effect | message |
|---|---|---|
| identify | identifies the type of the first entry in the pack, in letter order, whose potion or scroll type is not identified | that entry's inventory line, now identified (`d) 2 potions of healing`); with none, `You have nothing to identify.` |
| teleportation | the player moves to a free square anywhere, chosen as in **Levels** (which may be the square the player is on); nothing is picked up there | `You are somewhere else.` |
| magic mapping | every square remembered as a space is now remembered as its terrain (rock stays a space) | `A map forms in your mind.` |
| enchant weapon | the wielded weapon's bonus + 1 | `Your mace glows blue for a moment.` (the type's name), or with no wielded weapon `Your hands tingle.` |
| enchant armour | the worn armour's bonus + 1 | `Your ring mail glows silver for a moment.`, or with no worn armour `Your skin crawls.` |

The scroll leaves the pack and its type is identified before it takes
effect, so a scroll of identify never names itself.

## Seeing

The player **can see** a square, unless blind, when it is the player's own
square or one of its eight neighbours, or when the player is in a lit room
and the square is in that room's rectangle, walls included. The player is
**in** a room when the room's rectangle, walls included, holds the player's
square, so standing in a doorway counts as being in the room. Gone cells'
points are not rooms. In a dark room, or in a corridor, the player sees
only the neighbours.

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
player can see its square, or senses monsters.

## Monsters

Each monster on the level has a kind (an index into `data.MONSTERS`), a
position, hit points and their maximum, and a state, asleep or awake.
Monsters start asleep.

At the end of each turn the monsters act one at a time, in the order of the
level's monster list (the order they were created). A monster's action:

1. If its kind has the `regenerates` flag and its hit points are below
   their maximum, they go up by 1.
2. **Asleep:** if its kind has the `still` flag, nothing happens.
   Otherwise, if it is **near** the player, it wakes: at once if its kind
   has the `mean` flag, else if `below(3) == 0`. Waking is its whole
   action. A monster is near the player when it is on one of the player's
   eight neighbours, or the player is in a room (see **Seeing**) whose
   rectangle holds the monster's square, lit or dark.
3. **Awake** and `still`: nothing happens.
4. **Awake** with the `flies` flag: if `below(2) == 0` it flutters: it
   draws a direction `d = below(8)` from the list below and steps that way
   if the step is allowed and the target holds neither a monster nor the
   player; either way that is its whole action. Otherwise it acts as
   below.
5. **Awake:** if the player is on a neighbouring square and the step from
   the monster to the player is allowed, it attacks (see **Combat**).
6. Otherwise it moves one step closer to the player along the map: it takes
   the first direction, in the list below, whose step is allowed, whose
   target holds neither a monster nor the player, and whose target's
   **distance** is smallest and less than the monster's own. If there is
   no such direction, or the monster's square has no distance, it stays.

The directions, in order: west, south, north, east, north-west,
north-east, south-west, south-east (`h`, `j`, `k`, `l`, `y`, `u`, `b`,
`n`).

The **distance** of a square is the least number of allowed steps from the
player's square to it, counting steps through squares with monsters as if
they were empty. The player's square has distance 0; squares no path
reaches have none. The player does not move while the monsters act, so the
distances are the same for every monster in a turn.

Awake monsters always know where the player is: they come by the shortest
path, even from the other side of the level.

## Combat

An **attack roll** by an attacker of level `L` with bonus `B` against
armour `A` hits when `below(20) + 1 + L + B >= 10 + A`.

The player's **armour** is the worn armour's `armour` value (from
`data.ARMOURS`) plus its bonus, or 0 with none worn.

**The player attacks** a monster by moving into it. The monster wakes
(even if the attack misses). The player's attack roll uses the player's
experience level; as bonus, the strength bonus to hit plus the wielded
weapon's bonus; and the monster kind's `armour`. If it hits, the damage is
`roll(dice, sides)` of the wielded weapon's type's `damage` (`[1, 2]` with
no weapon), plus the weapon's bonus, plus the strength bonus to damage,
at least 0, and the monster loses that many hit points:

- at 0 or fewer it is removed from the level, the message is
  `You defeat the kobold.` (with the kind's name), and the player gains the
  kind's `xp` (see **Experience**);
- otherwise the message is `You hit the kobold.`

If the roll misses, the message is `You miss the kobold.`

| strength | to hit | damage |
|---|---|---|
| 6 or less | -1 | -1 |
| 7 to 16 | 0 | 0 |
| 17 | 0 | +1 |
| 18 or more | +1 | +2 |

When the player, not blind, attacks a monster whose kind has the
`paralyses` flag (the floating eye) and the monster is still on the level
after the attack, the message `You are frozen by the floating eye's gaze!`
follows and the player is frozen for `below(4) + 2` more turns (see
**Commands**).

**A monster attacks** the player with an attack roll of the kind's `level`,
bonus 0, against the player's armour. If it misses, the message is
`The kobold misses.` If it hits, the damage is `roll(dice, sides)` of the
kind's `damage` (no draws for `[0, 0]`), the player loses that many hit
points, the message is `The kobold hits you.`, and then:

- if the player's hit points are 0 or fewer, the player dies (see
  **The end**), killed by the kobold;
- otherwise, if the kind has the `stealsGold` flag (the leprechaun) and the
  player has gold, it steals `below(50 + 10 * depth) + 2` gold (all of it,
  if the player has less) and is removed from the level, with the message
  `Your purse feels lighter.`;
- otherwise, if the kind has the `stealsItem` flag (the nymph) and the pack
  has entries that are neither wielded nor worn, it steals one whole entry,
  `entries[below(len)]` of those in letter order, and is removed from the
  level, with the message `The nymph stole 2 amber potions!` (the entry's
  name and count).

A monster removed in its own action is skipped over: the next monster in
the list acts next.

## Experience

Experience points gained are added to the player's total. Then, while the
total is at least the next level's threshold, the player goes up a level:
the level rises by 1, `roll(1, 10)` is added to both the hit points and
their maximum, and the message is `Welcome to level 2.` (with the new
level). Level 2 needs 10 points, and each level after needs twice the one
before: 20 for level 3, 40 for level 4, and so on.

## Hunger

The player's **food counter** starts at 1300. In the upkeep at the end of
each turn it goes down by 1, and then:

- if it is now exactly 150: `You are starting to get hungry.`
- if it is now exactly 50: `You are starting to feel weak.`
- if it is -850 or less, the player starves (see **The end**), and the
  rest of the upkeep does not happen;
- otherwise, if it is 0 or less and the player is not frozen, then if
  `below(5) == 0` the player faints: frozen for `below(8) + 4` turns, with
  the message `You faint from lack of food.` (No draw while frozen.)

The status line shows `Hungry` while the counter is 150 to 51, `Weak` from
50 to 1, and `Faint` at 0 or less.

## Healing

The player heals slowly. In the upkeep, after hunger: if the player's hit
points are below their maximum, a **healing counter** goes up by 1, and
when it reaches `max(3, 21 - 2 * level)` (the experience level), the hit
points go up by 1 and the counter goes back to 0. At full hit points the
counter is 0.

## The end

**Death.** When the player dies, no more monsters act that turn, and the
messages `You die.`, then `Killed by the kobold on depth 1.` (the
monster's name and the depth) or, when starving, `Starved to death on
depth 1.`, then `Press enter to play again.` follow. The turn still ends:
the count goes up and the player looks around.

**Winning.** Taking the stairs on depth 8 with the Orb in the pack wins
the game, with the messages `You escape with the Orb and 123 gold.` (the
player's gold) and `Press enter to play again.`; no turn passes.

Either way the game is then over. Once its messages have been shown (see
**Messages**), only `enter` does anything: it starts a new game with the
seed one more than this game's.

## Messages

A command's messages are shown in the order they happened, joined with two
spaces. If together they are longer than 80 characters, the message line
shows only the first `n` of them followed by ` --More--`, with `n` the
largest number (at least 1) for which that line is 80 characters or less.
While `--More--` is showing, only `space` does anything: it shows the next
messages by the same rule. Other keys are ignored and no turn passes. When
the last messages are showing, the game takes commands again.
