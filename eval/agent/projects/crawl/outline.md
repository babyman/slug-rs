# Crawl — spec outline

Design outline for the next agent project: a small seeded roguelike on
`slug.arcade`, graded exactly. This is the design document, not the agent's
`task.md`; the task is written from it once the reference exists.

Goals, from the Invaders lessons:

- Harder than Invaders: more state, real data structures and algorithms, and
  a game that is not a transcription of a known program.
- Partial credit that stays local: one wrong rule fails the cases that use
  it, not every frame after its first use.
- No spec ambiguity: every rule has a worked example generated from the
  reference, and a second, independent implementation agrees with it.
- A continuous metric (the bot task) as well as a pass rate, to separate
  arms.

## Layout

Mirrors `projects/invaders`:

```
crawl/
  task.md            agent-facing spec (written last, from this outline)
  outline.md         this file
  make_cases.py      generates cases.json from the reference
  cases.json
  tools/make_data.py builds workspace/data.slug (font from the vendored
                     raylib's rtext.c, tables, names)
  workspace/
    data.slug        font, monster/item tables, names; read-only to agents
  reference/
    crawl.slug       entry point
    game.slug        rules (one module per area if it grows: gen, fov, ai…)
    bot.slug         reference bot (case generation and the bot baseline)
  python/
    task.md          the same task for the `python` control arm
    reference/       independent Python implementation, written from the
                     spec only: agreement with the Slug reference is the
                     ambiguity check, and it is the control arm's reference
```

## The machine

- Display: `slug.arcade`, 640×240 bits (80×24 cells of 8×10, one byte per
  glyph row, so a framebuffer row is 80 glyph bytes), `scale: 2`, no
  rotate. Static colour overlays for the message line and status line.
- Font: raylib's built-in default font (zlib licence, already vendored in
  the arcade clutch). raylib draws text on the GPU, outside the bits that
  are presented and graded, so the game does not call it; instead
  `make_data.py` decodes the glyph bitmap embedded in `rtext.c`
  (`LoadFontDefault`: 10 px high, proportional widths 1–7) and centres each
  ASCII 32–126 glyph in an 8×10 cell. Fallback if the proportional glyphs
  read badly on the map: the public-domain font8x8 set in 8×8 cells.
- Screen: row 0 messages, rows 1–22 the map (80×22), row 23 the status
  line: `Level:3  Gold:45  Hp:12(15)  Str:16  Arm:4  Exp:3/42  Hungry`.
  Exact column formats in the spec.
- The game draws characters into a cell grid and renders the whole grid to
  bits after each turn (about 5 ms, measured).
- Turns, not frames: the game advances one turn per key **press**
  (`arcade.pressed`), and presents every frame. Frame grading still works:
  scripts press a key for one frame and release it.
- Keys (limited to `arcade.keys`; no `>` or `.`):
  - `h j k l y u b n` and arrows: move/attack (Rogue directions)
  - `space` rest one turn, `s` search, `enter` descend stairs
  - `e` eat, `q` quaff, `r` read, `w` wield, `a` armour (wear/take off),
    `d` drop, `i` inventory; each item command prompts
    `Quaff which? (a-z)` and takes a letter key, `space` cancels
  - `shift` + direction: run until something interesting (spec lists the
    stop conditions exactly)
- Program arguments (needs the grader extension below):
  - `slug crawl.slug --seed N` starts a new game
  - `--load file.json` starts from a snapshot
  - `--dump file.json` writes the final state when the run ends

## State and the snapshot format

One JSON document is both the save format and the graded state dump. It holds
everything the rules read, so a case can start anywhere:

- `rng`: the generator state (one 32-bit integer, see RNG)
- `turn`, `depth`, `seed`
- `map`: 22 strings of 80 characters (terrain only), `seen`: the same shape
  as `0`/`1`, `rooms`: rectangles with `lit`
- `player`: position, hp, max hp, str, max str, level, xp, gold, hunger,
  wielded/worn letters, status timers (confused, blind, paralysed, fast)
- `inventory`: letter → item; `floor`: position → item
- `monsters`: list in creation order: kind, position, hp, state
  (asleep/wandering/chasing/fleeing), timers, carried item
- `names`: the unidentified-name assignment for potions and scrolls, and
  which kinds are identified
- `messages`: pending messages (for `--More--`)

The spec gives the field list and types; the grader compares dumps as
parsed JSON and prints the first differing path, which is what triage needs.

## RNG

The spec defines the generator (decided 2026-10-23), because
`slug.random.generator` keeps its state hidden and a snapshot must carry
it: xorshift32 (same as `slug.random`), `below(n)` with the same rejection
rule, `roll(n, d)` as n calls to `below(d) + 1`. About ten lines for the
agent to write, and the state is a plain integer in the snapshot. Agents
must not use `slug.random`; its sequence matches, but its state cannot be
saved.

**Draw order is a rule.** The spec has a table of every random draw and when
it happens, for example:

- each turn: player action draws, then monsters act in creation order, each
  drawing to-hit then damage; then the wandering-monster check; then regen
- level generation: room sizes cell by cell in row-major order, then
  connections, then stairs, then items, then monsters
- new game: shuffle potion names, then scroll names, then level 1

## Rules (v1 scope)

Each section of `task.md` gets the exact rule plus a worked example produced
by the reference (snapshot in, keys, screen + dump out).

1. **Level generation.** 3×3 grid of 26×7 cells; each cell is a room
   (random size and position inside the cell, lit unless a depth-based
   draw says dark) or a gone room (a single corridor junction). Connect
   neighbours with a random spanning tree plus extra edges, carve L-shaped
   corridors with doors at room walls. Stairs in a random room. Requires a
   connectivity check the agent will likely write as flood fill.
2. **Field of view.** Rogue's model rather than shadowcasting, so it is
   exact and short: inside a lit room you see the whole room; elsewhere the
   eight neighbours. Seen terrain is remembered; monsters and items show only
   while visible.
3. **Movement and running.** Diagonal moves blocked through doorways;
   running stops at the listed conditions.
4. **Monsters.** ~10 kinds in `data.slug` (letter, hp dice, armour, damage,
   xp, speed, flags), e.g. bat (erratic), jackal, kobold, snake, orc,
   floating eye (paralyses on hit), nymph (steals an item and teleports),
   troll (regenerates), leprechaun (steals gold). Awake monsters step along
   a shortest path to the player: BFS over walkable cells from the player,
   ties broken by a fixed direction order. Sleeping monsters wake on a draw
   when the player is in the same room.
5. **Combat.** to-hit: `below(20) + 1 + level + weapon bonus >= 21 - armour`
   (shape only; numbers decided in the reference); damage dice by weapon and
   strength bonus. Messages exactly: `You hit the orc.`, `The orc misses.`
6. **Items.** Gold, food, 3 weapons, 3 armours, 6 potions, 5 scrolls with
   shuffled unidentified names (`a fizzy potion`, `a scroll titled "zelgo
   mer"`). Effects: healing, poison, confusion, blindness, gain strength,
   see monsters; identify, teleport, magic mapping, enchant weapon, enchant
   armour. Inventory letters assigned lowest-free, stacking for food and
   identical potions/scrolls.
7. **Hunger and regeneration.** Counter, `Hungry` / `Weak` / `Faint`
   thresholds, fainting skips turns, starvation kills.
8. **Levels and experience.** XP table, hp gain dice per level.
9. **Messages.** A turn's messages are joined with two spaces; if they do
   not fit in 80 columns, show the first part with ` --More--` and wait for
   `space` (other keys ignored). This modal path is a deliberate trap.
10. **End.** Death shows a tombstone screen (`killed by a troll on level 6`
    with gold); reaching the stairs on level 8 with the Orb (generated on
    level 8) wins. Any key after either restarts with the next seed.

Out of scope for v1: throwing, wands, rings, traps, saving to disk from the
game, the hi-score table. These are the extension list if v1 proves too
easy.

## Cases

All generated by `make_cases.py` from the reference; each case is a starting
point (seed or snapshot), a key script, and expected frame-window digests
plus the final dump.

| group | start | what it pins down | approx. count |
|---|---|---|---|
| gen | `--seed N` at depth 1..8 (snapshot with only `rng` and `depth`) | level generation and draw order, first screen | 24 |
| fov | snapshots in lit room, dark room, corridor, doorway | visibility and memory | 8 |
| move | snapshots | diagonals at doors, running stops | 10 |
| combat | snapshots, one monster each | to-hit/damage/messages per kind | 12 |
| ai | snapshots with paths around corners | BFS steps and tie order | 8 |
| items | snapshots with chosen inventories | each effect, identification, stacking, prompts, cancel | 20 |
| hunger | snapshots near thresholds | states, fainting, starving | 5 |
| more | snapshots producing long message turns | `--More--` | 4 |
| long | `--seed N`, random keys, 2000+ turns | everything together | 6 |
| bot | `--seed N`, reference bot's keys | deep play, level 8, win | 4 |

Snapshot cases are where partial credit comes from; the long and bot cases
check that nothing drifts over thousands of turns.

## Bot task (second metric, after the pilot)

Deferred until the main task has been piloted (decided 2026-10-23). A
separate task: write `bot.slug`, a function from the visible screen (24
strings) and status to a key. The harness runs the **reference** game with
the agent's bot on 20 hidden seeds, 5000 turns each, and scores mean
`100 × depth reached + gold`, with a win worth a fixed bonus. Separate task
so it can ship the reference game without leaking the main task's answer,
and so it can run against the agent's own game later if wanted.

## Harness work

- `arcade_grader.py`: per-case program arguments and input files (the
  snapshot), plus comparing the `--dump` JSON with a first-difference report.
- A bot grader that imports the agent's `bot.slug` into the reference game
  loop and returns the score instead of pass/fail.
- `run.py`: the new task names; nothing withheld (`slug.fmt.json` must be
  available for the snapshot format, so check it is not in
  `WITHHELD_MODULES` for this task).

## Order of work

1. Done 2026-10-23: `make_data.py` font extraction; a sample screen reads
   well (the glyph cells leave `-` walls dashed, as in Rogue).
2. Done 2026-10-23: render spike. A full redraw of the 24 text rows into
   640×240 bits (glyph bytes appended per pixel row, rows concatenated)
   takes about 5 ms in the release build, so the game can redraw the whole
   screen every turn and needs no dirty-cell tracking.
3. Done 2026-10-23: `data.slug` tables (as structs: a list of map literals
   loses per-field types in the checker), `data.py` for the Python arm.
4. Reference game, rule by rule, with a snapshot test per rule. Done
   2026-10-23: the RNG and level generation (`reference/rng.slug`,
   `level.slug`), their `task.md` sections, and the Python versions
   (`python/reference/level.py`), identical on 200 seeds × depths 1–8.
   Done 2026-10-23: commands, moving, running (straight lines; stops on a
   door, stairs, an item, or a visible monster), resting, stairs, seeing
   and memory, the screen text (`game.slug`, `python/reference/game.py`);
   20 bot-played games of 600 commands identical in state and screen. About
   6 ms per command in Slug, mostly the full-screen render.
   Done 2026-10-23: monsters (waking, fluttering, shortest-path chasing),
   combat, experience, death and restart, `--More--`; 20 bot-played games
   of 800 commands identical. Balance: the unarmed player dies often on
   depth 1; revisit once the starting kit exists.
   Done 2026-10-24: items (pickup, pack of 22 with stacking, names and
   identification, starting kit, e/q/r/w/a/d/i with prompts, potions,
   scrolls, nymph theft), hunger and fainting, healing, confusion,
   blindness, sensing, winning with the Orb. Arrow keys stay distinct
   from h/j/k/l (they are ignored at prompts). 20 bot-played games of
   2500 commands identical; `compare_references.py find` bisects a
   mismatch to its first command. Not yet exercised by the bot: a full
   pack (needs a snapshot case).
5. Write `task.md` from the reference, examples generated.
6. Python reference from `task.md` only (a fresh agent, or by hand); every
   disagreement is a spec fix or a Slug reference bug. Then `python/task.md`
   and checks for the control arm.
7. `make_cases.py`, grader extensions, both references pass all cases.
8. Pilot one run per arm; triage every failed `slug` invocation.
9. Later: bot task and its grader.

## Risks

- **Spec size.** Invaders' spec is 650 lines for a small ROM. Keep v1 to
  the list above; the twin implementation tells us when the spec is
  incomplete rather than the pilot runs.
- **Draw-order brittleness.** Snapshot cases limit the damage, but long
  cases will still fail wholesale on one misordered draw. That is fine as
  long as they are a minority of the score.
- **Too easy again.** If v1 saturates, the extensions (throwing, wands,
  traps, multiple save slots) are cheap to add on the same snapshot
  machinery.

## Decisions (2026-10-23)

1. RNG: defined in the spec, state saved in snapshots.
2. Font: raylib's default font glyphs, extracted into `data.slug`; the game
   draws text itself, as Invaders does.
3. Bot task: after the pilot.
4. The Python twin is also the `python` control arm's reference.
