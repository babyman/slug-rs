# Crawl

Crawl is a roguelike played in a web browser by everyone on one server, in
one shared world that goes on whether anyone is playing or not. It lives in
the `slug.games.crawl` clutch (`clutch/slug.games.crawl.clutch`), written in
Slug, and is served over `slug.net.http` with HTMX.

This guide describes the game as it plays now. The numbers in it are the
current tuning, not promises; the clutch's module documentation and tests
are the authority on exact rules. Why the world is built this way is
recorded in the decisions
[Crawl: a shared world](../decisions/2026-10-26-crawl-shared-world.md) and
[Crawl: a living world](../decisions/2026-11-01-crawl-living-world.md).

The eval's Crawl (`eval/agent/projects/crawl`) is a separate, single-player
game that this one grew from; nothing here applies to it.

## Running a server

From the repository root:

```sh
target/release/slug clutch/slug.games.crawl.clutch/play.slug --host=0.0.0.0
```

The server prints its address and the world's seed. Phones and other
computers on the network connect to the host machine's address on port 8080.
Write options as `--name=value`.

| Option | Default | Effect |
|---|---|---|
| `--db=PATH` | `crawl.db` | The SQLite database the world is saved in. |
| `--new` | off | Start a new world in the database, replacing the one it holds. |
| `--seed=N` | random | The seed of a new world; the same seed makes the same world. |
| `--size=N` | 32 | How many chunks across and down a new world is. |
| `--history=N` | 100 | Years of history a new world lives through before anyone comes. |
| `--region=N` | 20 | Ticks between the region layer's visits to each chunk. |
| `--empire=N` | 6000 | Ticks between steps of the empire layer, a season each (15 minutes). |
| `--host=H` | 127.0.0.1 | The address to listen on; `0.0.0.0` for every interface. |
| `--port=N` | 8080 | The port to listen on; 0 picks a free one. |
| `--tick=MS` | 150 | Milliseconds between ticks. |
| `--pace=N` | 4 | Ticks for each move of the monsters. |
| `--save=N` | 20 | Ticks between saves. |

The world is saved every few seconds and when the server stops on Ctrl-C or
SIGTERM; a second Ctrl-C stops it at once, unsaved. Started again on the same
database, the server carries the world on, and every player's browser finds
its character waiting.

## The world

- The world is a grid of chunks, 32 by 32 by default, each an 80 by 22 map.
  Depth 0 is the surface; the dungeon below it goes down without end.
- Walking off the edge of a map takes you into the next chunk: on the surface
  across any open square of the edge, underground through the one opening on
  that edge. Every chunk has stairs down (`%`) and every dungeon chunk stairs
  up (`<`). The world's outer edge is sea on the surface and solid wall below.
- The surface is open ground, scrub, woods, lakes and rocky hills that run on
  smoothly from chunk to chunk. Outdoors you see about 18 columns across and
  9 rows up and down, into the chunks beside yours; trees, rock and walls
  block sight.
- Below are rooms and corridors. A lit room shows all at once; in a dark
  room or a corridor you see only the squares next to you.
- Towns, forts and ruins each fill a surface chunk:
  - A **town** has walls with four gates and roads out, a market, an inn,
    houses, townsfolk and guards. The way down is outside its walls.
  - A **fort** is a palisade around a way down, with a barracks, an armoury,
    guards, a captain and a quartermaster.
  - A **ruin** is a town or fort that fell during the world's history:
    broken, overgrown walls, no one there, and a way down inside.
- The world runs on a tick of 150 milliseconds, whether anyone presses a key
  or not. Each player acts at most once a tick, monsters move every fourth
  tick, and a run takes one step a tick. A player whose page is closed stays
  in the world, but their chunk stands still unless someone present is in
  it.

## Your character

- A new character starts in the streets of the start town, nearest the
  middle of the world, with 12 health, 16 strength, 1300 food, a +1 mace in
  hand, +1 ring mail worn, and a food ration. Your name is at the top of the
  page; type a new one there and press Enter to change it.
- Move with `h` `j` `k` `l` `y` `u` `b` `n`, the arrow keys, or the pad on a
  touch screen. Shift, or Run on the pad, runs until something interesting
  happens. `enter` takes the stairs and `space` waits.
- Walking onto an item picks it up. The pack holds 22 entries; potions,
  scrolls and food of the same kind stack. A full pack says what it leaves.
- The pack's buttons use or drop each item; the keys `e` eat, `q` quaff,
  `r` read, `w` wield, `a` wear or take off, and `d` drop ask which item.
- Killing monsters gives experience. Each level needs twice the last (10, 20,
  40, ...) and adds 1 to 10 to your health and its maximum. Health comes back
  slowly, faster at higher levels.
- Each turn you act costs one food. At 150 you are hungry, at 50 weak; at 0
  and below you sometimes faint, and at -850 you starve.
- When you die, the world carries on: press `enter` to start again as a new
  character in the start town, under the same name.

## Fighting

- Walk into a monster to attack it. A blow lands when a d20 roll plus your
  level and your weapon's bonus (and 1 for strength 18 or more) reaches 10
  plus the monster's armour. It deals the weapon's dice (the mace's 2d4)
  plus its bonus, more for strength 17 and up.
- Monsters start asleep and wake when you come near: mean kinds always, the
  rest a third of the time. Awake, they come for the nearest player.
- Nineteen kinds live at different depths, each kind over a range of them;
  the deepest kinds live at every depth below 12.

  | Depths | Kinds |
  |---|---|
  | Surface | jackal, snake, bat, giant rat, wolf |
  | 1 to 3 | those, and kobolds, goblins, hobgoblins, giant ants, floating eyes, orcs, leprechauns, skeletons |
  | 4 to 6 | nymphs, zombies, ogres and trolls join; the wild kinds and kobolds thin out |
  | 7 to 9 | orcs, floating eyes, leprechauns, nymphs, zombies, trolls, skeletons, ogres, wraiths |
  | 10 and below | dragons join, rarely |

  Bats flit at random; a floating eye never moves but can freeze you when
  you strike it; leprechauns steal gold and nymphs an item, and both vanish;
  trolls heal as they fight; a wraith's blows can sap your strength; a
  dragon is rare, lives alone and never breeds, and is the strongest thing
  in the world.
- Potions: healing, poison, confusion, blindness, gain strength, and see
  monsters. Scrolls: identify, teleportation, magic mapping, enchant weapon,
  and enchant armour. Unknown potions and scrolls go by colours and strange
  titles until you use or identify them. Weapons and armour come with a
  bonus from -1 to +1.

## People

Characters are the orange `@`s. They stand where they are and are never
hostile. Walking into one opens their menu, which takes no time; `space`
goes back and closes it.

| Who | Where | What they do |
|---|---|---|
| Trader | Town markets | Sells food, potions of healing and scrolls of identify; buys anything but what you wield or wear, for half its price. |
| Quartermaster | Fort armouries | Sells and buys weapons and armour. |
| Innkeeper | Town inns | Sells a meal, which fills you, and a bed, which heals you; tells rumours. |
| Captain | Fort barracks | Posts bounties for the region and pays for kills made in it. |
| Guard, townsperson | Gates, houses | Talks. |

What you buy is known for what it is, and what a dealer offers for an
unknown potion gives its nature away. Prices depend on the faction that
holds the land (see below).

Walking into another player opens a menu too: Talk, a list of phrases, one
key each, or Give, an item or some gold, which they get at once. The box
above the game, or `/`, says up to 80 characters to everyone who can see
you. Players cannot attack each other.

## The living world

The world runs on three layers, all on the same tick.

**The dungeon** is everything above: players, monsters and items in the
chunks players are in.

**The region layer** keeps the monsters everywhere else. About every three
seconds it visits each chunk no one is in:

- Monsters breed, up to what a chunk holds: two on the surface, four at
  depth 1, rising to eight by depth 8.
- They die off, faster away from the depths they belong to, and when a chunk
  is crowded the weakest kind gives way.
- They wander to the chunks beside theirs, across edges and up and down
  stairs, and a wanderer may come to an empty chunk.
- Now and then something turns up to be found.

So a cleared chunk fills again in time, from its neighbours as well as from
what survived. Towns and forts never get monsters.

**The empire layer** is the peoples and their factions:

- Humans and dwarfs live above, kobolds, orcs (with hobgoblins and
  goblins), trolls (with ogres) and the undead (zombies, skeletons and
  wraiths) below. Jackals, bats, snakes, giant rats, wolves, giant ants,
  floating eyes, leprechauns, nymphs and dragons belong to no one.
- Eight factions hold the land, a region of four by four chunks at one band
  of depths: the surface, depths 1 to 3, 4 to 6, or 7 and below. Each has a
  name in its people's style and four traits, bold, aggressive, orderly and
  greedy, which shift as it wins, loses or keeps the peace.
- Each season, 15 minutes of play and four to a year, each faction earns
  from its land, grows stronger, and may settle wild land beside its own or
  fight for held land. Strength fades
  the farther a faction reaches from its home depths, so the shallow depths
  are a contested frontier.
- A people's monsters thrive where its faction holds the land and fade where
  another does, so the shallows under human land are quieter.
- Your kills of a people's monsters weaken its faction where you make them.
- Markets charge more while their faction is at war, and more or less as it
  is greedy or generous.
- A fort's captain posts bounties, a price a head, on the peoples below its
  region; generous factions pay more.
- The stats show the date, as "Late spring, year 76", and who holds the
  land you are in. Each new season is announced as it comes.

## History

A new world lives through 100 years, 400 seasons, of the empire layer
before anyone comes. In those years the peoples below may besiege the settlements beside
their land. Walls double a settlement's defence, but each siege it holds off
wears them down; one that falls is a ruin from then on. Most worlds lose
none to five settlements; the start town always stands. The dungeon under a
ruin, down to depth 3, holds what the fallen left behind.

The world remembers. Every fight goes into a chronicle of the latest news,
and the great events go into the annals for good: each siege, how many
times the settlement held and whether it fell, and any faction driven from
the last of its land. Innkeepers tell of both: "Wolfby fell to the Sootfang
Warren in the summer of year 53, after holding out six times since the
autumn of year 46."

Players nudge the world; they do not decide it.
