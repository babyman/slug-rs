# Crawl: An Endless, Shared World

Status: Accepted; the goal and tick length are open

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
  has stairs down and stairs up, so players who part can meet again.
- A chunk is generated from its own generator, seeded by mixing the world
  seed with its coordinate, so the same world seed always gives the same
  chunk however and whenever it is reached.
- Whatever two chunks share is decided by the link, not by either chunk:
  each opening on a shared edge, and each staircase pair, is seeded from the
  link's own coordinate. Generation takes these openings as fixed and joins
  them to the chunk's rooms with corridors, so both sides always agree.
- A chunk is generated when someone first comes near it and is then kept in
  memory with whatever players did to it. Storing only changes against the
  generated chunk is left until memory needs it.
- Monster strength and item tables scale with depth instead of stopping at
  depth 8.

### Two kinds of randomness

Generation draws only from chunk and link generators. Everything that
happens in play (combat, monster moves, spawns, effects) draws from one world
generator in tick order. A chunk therefore looks the same whoever reaches it
first, and a world seed plus the log of every player's actions replays a
session exactly, which is how the engine is tested.

### A server tick

- The world advances on a fixed tick. Each player queues actions; every tick
  takes at most one queued action from each player, applies them in a fixed
  order (by arrival), then moves the monsters.
- Only active chunks (those with a player in them, or next to one) take
  turns. The rest of the world is frozen until someone comes back.
- Running (Shift with a direction) becomes one step per tick until something
  interrupts it, as it does now. Prompts that take no game time, such as
  choosing an item or `--More--`, are a player's own state and do not wait
  for a tick.
- One task owns the world and runs the tick loop; HTTP handlers only queue
  actions and read views. The engine can therefore keep its state in module
  globals, written by that one task.
- The loop paces itself with `recv` timeouts on the action queue, so it needs
  no wall clock.

### Players

- A cookie identifies each browser's player. A player has a position (chunk
  and cell), stats, pack, conditions, message log, and memory of which cells
  they have seen in each chunk; the world holds terrain, items, and monsters.
- Each player sees their own field of view. Other players are drawn as
  `@` in a different colour, with their names in the In View panel.

### Pushing views

Other players and monsters move without a key press, so the server pushes
each player's game fragment after a tick that changed what they see, as
server-sent events through htmx's SSE extension. This needs
`slug.net.http` to send a response whose body arrives over time, such as a
body given as a channel, with no `Content-Length` and the connection held
open.

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
- The tick length, and whether a player with no queued action simply waits.

## Plan

Each step leaves a playable game:

1. Split the engine's state into a world and one player, still single
   player and still turn per key. (Done.)
2. Chunked, endless generation with constrained edges and two-way stairs.
3. Several players by cookie, each command still advancing the world at
   once.
4. Streaming responses in `slug.net.http`, the tick loop, and pushed views.
