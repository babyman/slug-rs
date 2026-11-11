# Crawl: An Endless, Shared World

Status: Accepted; the goal is open

## Context

`slug.games.crawl` plays the eval's Crawl: eight fixed-size levels of one
80 x 22 map each, generated in turn from one seeded generator, and one player
whose key presses each advance the game by a turn. The web front end holds
that one game in a global and redraws it after every key.

The aim is a world with no edge, outwards on each level and downwards, that
several people play at once.

## Decision

### The world is chunks

- The world is made of chunks, each one map the size of today's level, at a
  coordinate `(x, y, depth)`. Walking off a map edge enters the neighbouring
  chunk at the matching position on its opposite edge; there is no separate
  overworld.
- Stairs join a chunk to the one below it at the same `(x, y)`. Each chunk
  has stairs down (`%`), and below depth 1 stairs up (`<`), so players who
  part can meet again. Going down arrives on the lower chunk's stairs up,
  and going up on the upper chunk's stairs down, so each chunk places its
  own stairs.
- A chunk is generated from its own generator, seeded by mixing the world
  seed with its coordinate, so the same world seed always gives the same
  chunk however and whenever it is reached.
- What two chunks share is decided by the edge between them, not by either
  chunk: each edge has one opening, a corridor square on both borders, at a
  row or column seeded from the edge's own coordinate. Generation takes the
  four openings as fixed and joins each to the room in the cell beside it,
  so both sides always agree. Rooms in the left and top cells keep two
  squares clear of the border so that corridor always fits.
- Walking off the map is allowed only straight across, from an opening into
  the matching one, and not onto a monster.
- A chunk is generated when someone first enters it and is then kept in
  memory with whatever players did to it. Storing only changes against the
  generated chunk is left until memory needs it.
- A chunk that no one has been in for 2000 turns (about 5 minutes of ticks)
  gets fresh monsters and items when someone next enters it, drawn by the
  rules that first stocked it from a seed that counts its restockings.
  Terrain stays, so players' maps stay right, and items players dropped
  stay; a chunk with anyone in it, even someone away, is never restocked.
- Below depth 8 monsters come from depth 8's table, until monster strength
  and item tables are made to scale with depth.
- The Orb and winning are gone until the game has a new goal.

### Two kinds of randomness

Generation draws only from chunk and link generators. Everything that
happens in play (combat, monster moves, spawns, effects) draws from one world
generator in tick order. A chunk therefore looks the same whoever reaches it
first, and a world seed plus the log of every player's actions replays a
session exactly, which is how the engine is tested.

### A server tick

- The world advances on a fixed tick, 150 milliseconds by default. Each
  player queues actions, at most four ahead; every tick takes at most one
  queued action from each player and applies them in a fixed order (by
  arrival). Monsters move on every fourth tick, so a player can move about
  four times for each move of theirs: with monsters moving every tick, a
  player who stopped to think was dead in seconds.
- Only active chunks, those with a present player in them, take turns. A
  player is present while their page is open (has an event stream); one
  whose page is closed is away, and stays in the world without keeping
  their chunk running. The rest of the world is frozen until someone comes
  back.
- A player who sends no action in a tick does not spend a turn: monsters
  still move, but the player does not get hungry or heal. A frozen player
  loses a turn each tick until they thaw.
- Keys that take no game time, such as opening an item prompt, still wait
  for a tick; the prompt's question stays above the map while the world
  moves.
- Running (Shift with a direction) becomes one step per tick until something
  interrupts it, as it does now. Prompts that take no game time, such as
  choosing an item or `--More--`, are a player's own state and do not wait
  for a tick.
- One task owns the world and runs the tick loop; HTTP handlers only queue
  actions and read views. The engine can therefore keep its state in module
  globals, written by that one task.
- The loop sleeps between ticks with `select { after ... }`, so it needs no
  wall clock; ticks drift by however long each takes.
- The engine keeps its turn-per-key `command` beside `tick`, for tests and
  replay; with one player its games are byte for byte as before.

### Players

- A cookie identifies each browser's player. A player has a position (chunk
  and cell), stats, pack, conditions, message log, and memory of which cells
  they have seen in each chunk; the world holds terrain, items, and monsters.
- Each player sees their own field of view. Other players are drawn as
  `@` in a different colour, with their names in the In View panel.
- Players block each other and monsters, and a new character never starts
  on another player.
- Each monster goes for the nearest living player in its chunk, so a player
  can be hit, or die, on someone else's turn. A player who stops playing
  stays in the world, where monsters can still find them.
- The cookie's id is 128 random bits from `slug.random.secure`, the
  operating system's secure generator, so no one can guess another player's.
  The world's seed comes from it too unless one is given.

### Pushing views

Other players and monsters move without a key press, so the server pushes
each player's game fragment after a tick that changed what they see, as
server-sent events that a few lines of the page's script swap in. Key posts
are answered `204`. `slug.net.http` sends a response whose body is a
channel as it arrives, with no `Content-Length`, closing the connection when
the channel closes; it closes the channel when the client goes away. A
keep-alive comment every 60 ticks finds streams whose page has gone.

## Consequences

The engine's game rules (combat, items, monsters, field of view) carry over;
what changes is where state lives (a world and players instead of one game),
level generation (constrained edges and stairs, and seeds per chunk), and the
turn loop. The eval's Crawl, with its snapshot grading, stays as it is; this
work is in the clutch only.

Replay by world seed and action log keeps the engine testable without a
browser or a clock. Holding every visited chunk in memory bounds a world by
how much of it players explore.

`slug.net.http` gains streaming responses, which any server that pushes
updates can use.

### Starting, dying, and restarting

- New players start at a random place on depth 1, the top level.
- A player who dies starts again as a new character at a random place on
  depth 1; the world, and whatever they left in it, carries on.
- The world lives only in the server's memory: restarting the server starts
  a new world.

## Open Questions

- What a game is for once the Orb on depth 8 is gone. The direction is a
  goal that is random or that emerges from the world, rather than a fixed
  depth.

## Plan

Each step leaves a playable game:

1. Split the engine's state into a world and one player, still single
   player and still turn per key. (Done.)
2. Chunked, endless generation with constrained edges and two-way stairs.
   (Done.)
3. Several players by cookie, each command still advancing the world at
   once. (Done.)
4. Streaming responses in `slug.net.http`, the tick loop, and pushed views.
   (Done.)
