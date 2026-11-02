# Crawl: A Living World

Status: Accepted

## Context

`slug.games.crawl` is now one shared world of chunks, `(x, y, depth)`, endless
outwards and downwards, run on a 150 millisecond tick for the chunks players
are in (see `2026-10-26-crawl-shared-world.md`). Everywhere else is frozen.
A chunk no one has visited for a while is restocked from fixed tables, and
the world is lost when the server restarts. The game has no goal since the
Orb went.

The aim is a world that goes on without the players: factions with
territory, settlements and an economy, changing over hours and days, around a
dungeon that still plays at the speed of a key press. The goals players
follow come from that world rather than from a fixed depth.

## Decision

### Three layers, each on its own clock

| Layer | Runs | Simulates | Unit |
|---|---|---|---|
| Dungeon | every tick (150 ms), active chunks only | monsters, combat, items | squares of a chunk |
| Region | every few seconds | monster populations, lairs, migration, loot levels | chunks, as counts |
| Empire | about once a minute, or slower | factions, territory, settlements, resources, trade, war | regions of chunks |

The rates are starting values, set on the command line like `--tick` and
`--pace`.

- The layers share only summaries. Going down, a slower layer sets the
  terms of a faster one: the faction that holds a region decides which
  monsters live in it, how much loot there is, and what its markets pay.
  Going up, a faster layer reports events to the slower one: players killed
  thirty goblins in a region, or sold twelve ore in a town. No layer reads or
  writes another's state directly, so each can be tested alone.
- One clock drives all three: the region layer runs every N dungeon ticks
  and the empire layer every M region steps. A slow step that costs too much
  for one tick is spread across the following ticks.
- Each layer draws from its own seeded generator. A world seed plus the log
  of player actions still replays the whole world exactly, which keeps the
  engine testable without a browser or a clock. Running the layers as
  separate tasks on their own timers was considered and rejected because it
  loses replay.

### A bounded world with a surface

- The world is bounded across: a fixed number of regions in x and y (the
  size is a world setting), each region a square of chunks. Depth stays
  unbounded. The map's outer edges have no openings.
- Depth 0 is the surface: outdoor chunks of terrain (open ground, forest,
  hills, water, roads) on the same chunk grid, joined by the same edge
  openings, and by stairs into the dungeon below. This replaces "no separate
  overworld" in the shared-world decision: the surface is not a separate
  map, just the top layer of chunks.
- New characters start on the surface, in or near a settlement.
- Regions are the empire layer's map. Each has a terrain type and resources.
  Territory is a region at a band of depths, so one region can be held by
  humans on the surface, contested at depths 1 to 3, and held by orcs below.

### Above and below

Factions live both on the surface and underground, in a natural order:
lawful and good(ish) peoples above (humans, dwarfs), evil and chaotic ones
below (orc hordes, trolls). The order is a tendency, not a rule.

- Each faction has a home depth, and its strength falls the further it
  reaches from home: supply lines, light, unfamiliar ground. Raids and
  incursions in either direction are possible but costly to hold.
- So a frontier forms of itself in the shallow depths, where the two sides
  meet and territory changes hands. Most contracts and most danger come from
  there.
- A strong faction can take a foothold far from home: an orc camp on the
  surface, a dwarf hold at depth 6, a town overrun. Footholds that fall are
  where ruins come from.
- Alignment sets home depth and default allies, not friendship. Factions on
  the same side still compete, humans and dwarfs over trade and mines, orcs
  and trolls over the same tunnels.
- A faction's people are what fills its territory: an orc-held band stocks
  its chunks with orcs and their kind, a dwarf-held one with dwarf patrols
  and miners.

### Peoples, factions and personalities

- A people is a kind of being with a classic fantasy alignment on two axes,
  lawful to chaotic and good to evil, and a home depth: humans and dwarfs
  above; kobolds, hobgoblins and orcs, trolls, and the undead below. A
  faction belongs to one people, and a people can have several factions,
  which may be rivals.
- Each faction has a generated name in its people's style (place names,
  titles, a banner word: "the Iron Compact of Thornhold", "the Blackmaw
  Horde") and a personality: a few traits on scales, such as bold to
  cautious, aggressive to peaceful, orderly to wild, and greedy to
  generous.
- Alignment says what a faction is; personality says what this one does.
  Traits weight the empire layer's choices: a bold faction raids further
  from home, an aggressive one goes to war where a peaceful one trades, an
  orderly one keeps treaties and borders and a wild one splits and fights
  itself, a greedy one taxes and hoards. Two human factions can therefore
  behave very differently.
- Traits move along their scales as things happen to a faction: losing a
  region, being raided again and again, a long peace, a rich trade. A
  horde beaten back may turn cautious; a town under constant attack may
  turn aggressive. Changes are slow and driven by empire layer events, so
  players' nudges reach personalities too.
- Only traits that visibly change what a faction does are kept.
- Rumours, contracts and characters speak in their faction's personality,
  so players learn who they are dealing with.

### Monsters by kind

Monsters follow familiar RPG lines.

- Kinds that live in groups belong to a people and so to its factions:
  kobolds, hobgoblins and orcs, trolls, zombies and other undead. They
  appear where their faction holds territory, and in raids and patrols.
- Solitary and wild kinds belong to no faction: dragons, rats, bats,
  snakes, jackals, floating eyes, and fey such as leprechauns and nymphs.
  They live wherever the region layer's populations put them, nest in
  places no faction holds, and trouble every side. A dragon is a solitary
  power of its own in the region layer, and not part of any empire.
- The current eleven monsters are the starting set; the table grows with
  the peoples (dwarfs, humans and their characters) and the solitary kinds
  (rats, dragons).

### History before players arrive

Creating a world runs the empire layer for a few hundred simulated years
before anyone joins. Factions rise, expand, trade, fight and fall, so players
arrive in a world that already has borders, roads, ruins and grudges.

- Settlements that fell during history become ruins: surface chunks of broken
  walls over dungeons that are deeper and better stocked than usual.
- History is kept as a short chronicle of events. Inns draw rumours from it
  and from what is happening now.
- History must run in seconds, so the empire layer works only on counts and
  tables, never on chunks.

### Settlements

- Towns and forts are places on the surface, each one or more chunks with
  hand-shaped layouts (walls, gates, streets, buildings) generated from the
  settlement's size, owner and wealth. They are walkable like any chunk.
- A town is for trade and talk: a market that buys loot and sells supplies,
  and an inn for rumours, rest and contracts. A fort is military: guards,
  patrols that leave it, bounties, and few shops.
- Their people depend on the owning faction and its state. A town at war has
  more guards and higher prices; a starving town has empty stalls.
- Market prices come from the empire layer's supply and demand for each
  good in that settlement.

### Characters

- A new kind of being, a character, sits beside monsters and players:
  traders, innkeepers, guards, travellers. Characters belong to a faction
  and are not hostile unless their faction is hostile to the player.
- Walking into a character starts an interaction instead of an attack: a
  menu of what that character offers (talk, buy, sell, take a contract).
  Interaction menus use the same mechanism as item prompts, and they take
  no game time while open.
- The same mechanism handles players meeting: walking into another player
  offers talk and trade instead of blocking. Attacking a character or a
  player needs a deliberate choice, never a bump.
- Players talk both ways: a menu of fixed phrases, one key each, and free
  text, which everyone in sight hears. Trading starts as giving, one way: an
  item or gold, with nothing to accept; two gifts make a trade. Players
  cannot attack each other yet.

### Players nudge, they don't decide

Player events move the empire layer by small amounts: kills thin out a
region's monsters and lower the strength of the faction they belong to, sales
change a town's supply, completed contracts give a little to the faction
that offered them. A war is decided by the factions' own strength, not by
players, though many players acting together over time can tip a close one.
How much each event counts is a set of tuning values, starting small.

### Goals come from the world

Factions want things, and say so through contracts at forts and inns and
through prices at markets: bounties on a region's monsters, ore wanted at a
forge, an escort for a caravan, a ruin to clear. These change as the world
does, and everyone in the world sees the same ones. This answers the shared
world decision's open question.

### Regions stock the dungeon

The region layer keeps, for each chunk, counts of the monster kinds that live
there. Populations breed, die off, fight neighbouring kinds and spread
through openings and stairs. When a player enters a chunk, its monsters are
placed from those counts, and when they leave, the survivors go back into
the counts. This replaces restocking by idle time.

### The world is saved in SQLite

- The world lives in a SQLite database through `slug.db.sqlite`, its first
  real use. A server started on an existing database resumes that world.
  A new world needs `--new` (or a new database path), so a restart never
  wipes a world by accident.
- Chunks, players, regions, factions and settlements are rows, each holding
  JSON (`slug.json`) for its state, with columns only for what is looked up
  by key. Chunks are written when they change and leave the active set;
  everything else is written at each empire step, and on shutdown.
- The database is the store, not the simulation: the world is still held
  and run in memory, so the tick never waits on disk.

## Consequences

- The shared-world decision changes in four places: the world is bounded
  across, depth 0 is a surface, restocking by idle time is replaced by
  region populations, and the world survives restarts.
- Replay by seed and action log still holds, now across three layers, so
  the engine stays testable without a browser or a clock.
- `slug.db.sqlite` becomes a dependency of the game. It links the system
  `libsqlite3`; only the macOS build has been used, so Linux needs checking,
  and the eval's Crawl, which does not use it, is unaffected.
- Characters and interaction menus are new work in the engine and the UI.
  They are needed for players meeting anyway.
- Generation grows from one dungeon generator to several: surface terrain,
  settlements, ruins and dungeons, each on chunk-seeded generators.

## Open Questions

- How far from home a faction's strength reaches, and so how deep the
  contested band is. The first version halves it for each band away from
  home (all of it at home, then 50, 25 and 10 percent).
- Leaders as characters players can meet, and fight: killing a faction's
  leader would shift its personality and strength. Wanted, but after the
  empire layer works.
- How large a world is by default, in regions and chunks per region.
- What the economy's goods are, and how many: few enough to balance, enough
  for trade to matter. The first version has none: each faction has one
  wealth, which its land earns and its strength costs.
- Whether players can hold anything: a house, a stall, a share in a town.
- Whether players can attack each other, and where: not yet (decided
  2026-11-01); perhaps later with factions and the law in towns.

## Plan

Each step leaves a playable game:

1. A bounded world with a surface: depth 0 terrain chunks, closed outer
   edges, stairs down from the surface, new characters on the surface.
   (Done.)
2. Saving the world in SQLite, resuming it on start, `--new` for a new one.
   (Done.)
3. Characters and interaction menus, used first for players meeting each
   other (talk, trade). (Done for players: menus, talk and giving; the
   characters themselves come with towns.)
4. Towns and forts as walkable chunks with fixed people: a market with fixed
   prices and an inn with fixed rumours. (Done.)
5. The region layer: monster populations per chunk, spread and breeding,
   dungeon stocked from counts instead of restocked. (Done: the chunks
   made, and the chunks beside them as counts; loot turns up too. Lairs
   wait for the empire layer.)
6. The empire layer: factions, territory, resources, production and trade;
   market prices and contracts from it; player events nudging it. (Done,
   first version: one wealth number for each faction instead of goods,
   standing bounties at forts as the contracts, and players see only who
   holds the land they are in.)
7. History at world creation: the empire layer run for a few hundred
   years, ruins from fallen settlements, rumours from the chronicle.
   (Done: 300 years by default, `--history=N`. Only in history do factions
   from below besiege settlements, and every siege a settlement holds off
   wears its walls down; the start town never falls. The dungeon under a
   ruin is richer, down to depth 3, but no deeper than elsewhere.)
