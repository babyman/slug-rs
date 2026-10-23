# Space Invaders

Write `./invaders.slug`, a program that plays Space Invaders, the 1978
arcade game, on `slug.arcade`. The game logic is the original machine's
program, described in this specification. It draws into the machine's 1-bit
video RAM exactly as that program does, and the hidden tests compare the
video RAM shown in every frame with a reference implementation's. So
everything below is exact: positions, timings, the order of steps, and the
original's quirks.

You may split the program into modules beside `invaders.slug`. `./data.slug`
holds the original ROM's sprites, font, tables, and messages; import it with
`val rom = import("data")` and do not change it (the tests supply their own
copy). `slug.arcade`'s documentation is the comment at the top of
`slug-docs/library/arcade/arcade.slug`.

## Running and testing

`invaders.slug` opens the display, then runs one game frame per
`arcade.frame()` until it returns false, and then closes the display:

```slug
arcade.open("Space Invaders", 256, 224, {rotate: true, scale: 3})
arcade.overlay(0, 32, 224, 32, 0xff2020)
arcade.overlay(0, 184, 224, 56, 0x20ff20)
arcade.overlay(16, 240, 118, 16, 0x20ff20)
```

Before the first frame, define the game's ten sounds with `arcade.sound`
(see **Sound**). Each frame: read the keys with `arcade.down` (left:
`"left"`, right: `"right"`, fire: `"space"`, start: `"enter"`), run the
frame's game logic, make the frame's sound calls, `arcade.present` the video
RAM, and call `arcade.frame()`. Present exactly once per frame, starting
with frame 0.

You cannot open a window here. Run the game headless with an input script,
as `slug.arcade` describes: for example

```
SLUG_ARCADE_HEADLESS=script.txt SLUG_ARCADE_OUT=out slug invaders.slug
```

with a script such as

```
0
10 enter
14 -
330 right space
333 right
shot 600
end 601
```

writes `out/frame-600.png`, a screenshot you can look at. Headless runs make
no sound, but with `SLUG_ARCADE_SOUNDS=sounds.txt` they log every `play`,
`loop`, and `stop` call; the tests compare that log too. The tests run
scripts like this, up to 20000 frames long, with a time limit of 20 seconds
plus one second for every 150 frames, so the game must run at least that
fast.

## The machine

### Video RAM

The video RAM, `vram`, is 7168 bytes: 224 rows of 32 bytes. Present it as is:
`arcade.present(vram)` with the display opened as above shows row `r` as
screen column `r` (from the left), and bit `k` of byte `b` in a row as the
pixel `8 * b + k` up from the bottom of the screen. The screen is 224 pixels
wide and 256 high.

The original program addresses the video RAM as memory from 0x2400 to
0x3FFF. This specification writes screen addresses that way: address `A` is
`vram` offset `A - 0x2400`, so 0x2400 is row 0, byte 0, and 0x271C is row 24,
byte 28.

Game objects have positions `(yr, xr)`: `yr` is the height in pixels above
the bottom of the screen and `xr` is the screen column plus 32, so `xr` 32 is
column 0. A position's offset in `vram` is `(xr - 32) * 32 + (yr >> 3)`, and
its shift is `yr & 7`. All positions and counters are bytes: arithmetic on
them wraps modulo 256 unless a size is given.

Writes to offsets outside `vram` are ignored, and reads there give 0.

### Sprites

A sprite is a sequence of bytes, one per row (one screen column), with bit 0
its lowest pixel. A sprite of `n` bytes covers `n` consecutive rows. The
drawing routines, with `a` a `vram` offset and `pic` a sprite:

- **Draw simple** `(a, pic)`: for each `i`, byte `a + 32i` becomes `pic[i]`.
- **Clear small** `(a, n)`: for each `i < n`, byte `a + 32i` becomes 0.
- **Erase simple** `(a, n)`: for each `i < n`, bytes `a + 32i` and
  `a + 32i + 1` become 0.

The shifted routines take a position. For each `i`, with `p = pic[i] << s`
(`s` the shift), `lo = p & 255`, `hi = p >> 8`, and `a` the position's offset,
they change bytes `a + 32i` (with `lo`) and `a + 32i + 1` (with `hi`):

- **Draw** writes `lo` and `hi`.
- **Draw shifted** ORs them in.
- **Erase shifted** clears their bits: `old & ~lo`, `old & ~hi`.
- **Draw with collision** ORs them in, and sets the flag `collision` to 1
  when any of their bits was already lit, and to 0 otherwise.

Every routine reads the bytes it changes before writing any of them.

### Text

The font in `data.slug`, `font`, has 64 characters of 8 bytes. Character `c`
is `font[8c:8c + 8]`, drawn with draw simple at an address; the next
character goes at that address plus 256 (8 rows on). The character codes:
`A`-`Z` are 0x00-0x19, the digits `0`-`9` are 0x1A-0x23, `<` 0x24, `>` 0x25,
space 0x26, `=` 0x27, `*` 0x28, `-` 0x3F. The messages in `data.slug` are
already strings of these codes.

To print a number as `n` digits, print its decimal digits, most significant
first, with leading zeros. Scores are shown with 4 digits; the credit count
with 2.

### Screen operations

- **Clear the screen**: every byte becomes 0.
- **Clear the playfield**: in every row, bytes 2 to 27 become 0.
- **Draw the bottom line**: in every row, byte 2 becomes 1.

### Input and frames

The keys are read once at the start of each frame and hold for that frame.

The original machine has two interrupts per 60 Hz frame. Each frame runs, in
order:

1. the **mid-screen interrupt**,
2. a **foreground slot**,
3. the **end-of-screen interrupt**,
4. a **foreground slot**,

and then presents `vram`. The interrupts run the moving objects. The
foreground is the program's main sequence: its game loop, its waits, and its
screens. In each slot the foreground runs until it reaches a wait whose
condition is not yet met, or finishes one game-loop iteration (described in
**Foreground**). `beam` is 0 during the mid-screen interrupt and the slot after
it, and 0x80 during the end-of-screen interrupt and the slot after it. Some
objects act only in the interrupt that matches their position: an object at
`xr` matches when `(xr & 0x80) == beam`.

## State

The game's variables, with the values the **RAM reset** sets:

| Variable | Reset value | Meaning |
|---|---|---|
| `waitOnDraw` | 1 | the current alien has yet to be drawn |
| `alienIsExploding` | 0 | an alien is exploding |
| `expAlienTimer` | 0x10 | frames left in the alien explosion |
| `alienRow` | 0 | the current alien's row |
| `alienFrame` | 0 | the aliens' animation frame, 0 or 1 |
| `alienCurIndex` | 0 | the current alien, 0-54 |
| `refAlienDY`, `refAlienDX` | 0, 2 | the rack's next vertical and horizontal step |
| `refAlienYr`, `refAlienXr` | 0x78, 0x38 | the reference alien's position |
| `alienPosYr`, `alienPosXr` | 0x78, 0x38 | the current alien's position |
| `rackDirection` | 0 | 0 moving right, 1 moving left |
| `obj0Timer`, `obj0Extra` | 0x80, 0 | the player object's timers |
| `playerAlive` | 0xFF | 0xFF alive, otherwise the explosion picture, 0 or 1 |
| `expAnimateTimer` | 5 | frames until the next explosion picture |
| `expAnimateCnt` | 0x0C | explosion pictures left |
| `playerPic` | `player` | the player's sprite |
| `playerXr` | 0x30 | the player's `xr`; its `yr` is always 0x20 |
| `obj1Timer`, `obj1Extra` | 0, 0 | the player shot's timers |
| `shotStatus` | 0 | the player's shot, below |
| `shotBlowTimer` | 0x10 | the shot explosion's timer |
| `shotPic` | `playerShot` | the shot's sprite |
| `shotYr`, `shotXr` | 0x28, 0x30 | the shot's position |
| `fireBounce` | 0 | fire is held after firing |
| `obj2Timer`, `obj2Extra` | 0, 2 | the rolling shot's timers |
| `obj3Timer`, `obj3Extra` | 0, 0 | the plunger shot's timers |
| `obj4Timer`, `obj4Extra` | 0, 0 | the squiggly shot's (and saucer's) timers |
| `rolling`, `plunger`, `squiggly` | see **Alien shots** | the three alien shots |
| `collision` | 0 | set by draw with collision |
| `expAlienYr`, `expAlienXr` | 0, 0 | the exploding alien's position |
| `playerOK` | 1 | 0 while the player explodes |
| `enableAlienFire` | 0 | aliens may fire |
| `alienFireDelay` | 0x30 | player-object runs until aliens may fire |
| `invaded` | 0 | the aliens reached the bottom |
| `skipPlunger` | 0 | the plunger shot is retired |
| `alienShotDelta` | 0xFC | the alien shots' step, -4 |
| `shotSync` | 1 | which alien shot may act |
| `numAliens` | 0xFF | live aliens, counted by the game loop |
| `saucerStart` | 0 | a saucer is due |
| `saucerActive` | 0 | the saucer is on screen |
| `saucerHit` | 0 | the saucer has been hit |
| `saucerHitTime` | 0x20 | the hit sequence's timer |
| `saucerPic` | `saucer` | the saucer's sprite |
| `saucerXr`, `saucerDelta` | 0x29, 2 | the saucer's `xr` and step; its `yr` is always 0xD0 |
| `sauScoreIdx` | 0 | position in `saucerScoreTable`, 0-14 |
| `shotCount` | 0 | player shots ended |
| `tillSaucer` | 0x600 | a 16-bit countdown to the next saucer |
| `soundPort3` | 0 | the sound bits for port 3 |
| `soundPort5` | 1 | the sound bits for port 5: the next fleet note |
| `changeFleetSnd` | 1 | a fleet note was played: choose the next |
| `fleetSndCnt` | 0x40 | end-of-screen interrupts until the next fleet note |
| `fleetSndReload` | 0 | the delay between fleet notes |
| `fleetSndHold` | 0x10 | end-of-screen interrupts until the note is released |
| `extraHold` | 0 | game-loop iterations until the extra-ship sound ends |

The RAM reset does not change these, which power-on sets:

| Variable | Power-on value | Meaning |
|---|---|---|
| `isrDelay` | 0 | a countdown the end-of-screen interrupt decrements |
| `suspendPlay` | 0 | 1 when the interrupts run the game |
| `gameMode` | 0 | 1 during a game |
| `numCoins` | 0 | credits |
| `p1Score`, `hiScore` | 0, 0 | scores, 0-9999 |
| `adjustScore`, `scoreDelta` | 0, 0 | a pending score increase |
| `player1Ex` | 1 | the extra ship is still to be awarded |
| `aShotReloadRate` | 8 | how soon aliens fire again |
| `port3`, `port5` | 0, 0 | the values on the two sound ports |

The **data page** is 256 bytes, all 0 at power-on:

- bytes 0-54: one per alien, 1 alive and 0 dead. Alien `i` is in row
  `i / 11` (row 0 at the bottom) and column `i % 11` (column 0 at the left).
- bytes 0x42-0xF1: the shield buffer, four copies of `shield`.
- byte 0xFB: the rack's starting `refAlienDX`.
- bytes 0xFC, 0xFD: the rack's starting `refAlienYr` and `refAlienXr`.
- byte 0xFE: the rack count.
- byte 0xFF: the reserve ships.

Some calculations below can index the data page outside the alien bytes; they
use whatever is there.

## Interrupts

The **mid-screen interrupt**: when `suspendPlay` and `gameMode` are both
non-zero, run the game objects from object 1, then the **alien cursor**.

The **end-of-screen interrupt**: decrement `isrDelay`. Then, when
`suspendPlay` and `gameMode` are both non-zero: run the **fleet timer**, set
`shotSync` to `obj2Extra`,
**draw the alien**, run the game objects from object 0, and, unless an object
handler left the interrupt (only object 0 can), run the **saucer timer**.

### Game objects

Objects 0 to 4 have a 16-bit `timer` and an 8-bit `extra`. Running the
objects from object `k` takes each object from `k` to 4 in turn: when its
`timer` is not 0, decrement it; otherwise, when its `extra` is not 0,
decrement that; otherwise run its handler. When object 0's handler leaves the
interrupt, the objects after it do not run.

### Object 0: the player

When `playerAlive` is not 0xFF, the player is exploding:

1. Decrement `expAnimateTimer`. If it is not 0, stop.
2. Let `image` be `playerAlive`. Set `playerOK` to 0, `enableAlienFire` to 0,
   `alienFireDelay` to 0x30, `expAnimateTimer` to 5, and decrement
   `expAnimateCnt`.
3. If `expAnimateCnt` is not 0: set `playerAlive` to `(image + 1) & 1`,
   `playerPic` to picture `playerAlive` of `playerBlowups` (16 bytes each),
   draw the player, set `obj0Extra` to 0, and stop.
4. Otherwise the explosion is over: erase simple the player's position, 16
   rows. Reset the player object: `obj0Timer` 0x80, `obj0Extra` 0,
   `playerAlive` 0xFF, `expAnimateTimer` 5, `expAnimateCnt` 0x0C, `playerPic`
   `player`, `playerXr` 0x30. Turn the sound bits off with mask 0. If `invaded` is 0 and `gameMode` is not 0, the
   foreground goes on with **after a death**, and the handler leaves the
   interrupt.

Otherwise the player is alive:

1. Set `playerOK` to 1. If `enableAlienFire` is 0, decrement
   `alienFireDelay`, and set `enableAlienFire` to 1 when it reaches 0.
2. If right is held, increment `playerXr` unless it is 0xD9. Otherwise (right
   is not held), if left is held, decrement `playerXr` unless it is 0x30. So
   holding both keys at the right edge does not move the player.
3. Draw the player and set `obj0Extra` to 0.

To **draw the player**, draw simple `playerPic` at the offset of
`(0x20, playerXr)`.

### Object 1: the player's shot

The handler acts only when `shotXr` matches the beam and `shotStatus` is not
0:

- **1** (just fired): set `shotStatus` to 2 and `shotXr` to `playerXr + 8`,
  and draw shifted `shotPic` at `(shotYr, shotXr)`.
- **2** (moving): erase shifted `shotPic` at the shot's position, add 4 to
  `shotYr`, and draw with collision. If `collision` is set, set
  `alienIsExploding` to it.
- **3** (exploding): decrement `shotBlowTimer`. If it is 0, **end the shot**.
  Otherwise, if it is 0x0F: erase shifted `shotPic` at the shot's position,
  set `shotPic` to `playerShotExplosion`, subtract 2 from `shotYr` and 3 from
  `shotXr`, and draw shifted.
- **5** (an alien is exploding): nothing.
- any other value: **end the shot**.

To **end the shot**: erase shifted `shotPic` at the shot's position; reset
the shot: `shotStatus` 0, `shotBlowTimer` 0x10, `shotPic` `playerShot`,
`shotYr` 0x28, `shotXr` 0x30; advance `sauScoreIdx`, wrapping from 14 to 0;
increment `shotCount`. Then, if `saucerActive` is 0, set the next saucer's
direction from bit 0 of `rom0800[shotCount]`: when it is 1, `saucerXr`
becomes 0x29 and `saucerDelta` 2, otherwise `saucerXr` 0xE0 and
`saucerDelta` 0xFE. (The original reads this from its own code by
mistake.)

### Alien shots

The three alien shots each have these fields:

| Field | `rolling` | `plunger` | `squiggly` | Meaning |
|---|---|---|---|---|
| `status` | 0 | 0 | 0 | bit 7 active, bit 0 exploding |
| `step` | 0 | 0 | 0 | steps taken |
| `track` | 0 | 1 | 1 | 0: aims at the player |
| `cfir` | 0 | 0 | 6 | position in `columnFireTable` (the rolling shot uses it as a skip flag) |
| `blow` | 4 | 4 | 4 | the explosion's timer |
| `frame` | 0 | 0 | 0 | picture offset, 0, 3, 6, or 9 |
| `pic` | first 3 bytes of `rollingShot` | of `plungerShot` | of `squigglyShot` | the sprite drawn |
| `yr`, `xr` | 0, 0 | 0, 0 | 0, 0 | the position |

Resetting a shot gives it these values. A shot's handler copies it to the
**active shot**, runs **the active shot**, and then copies the active shot
back, or resets it, as described for each object. `otherShot1` and
`otherShot2` hold the other two shots' `step` values.

**The active shot**, given its picture strip `pictures` (`rollingShot`,
`plungerShot`, or `squigglyShot`):

If its `status` bit 7 is clear, it may fire:

1. If `enableAlienFire` is 0, stop.
2. Set `step` to 0.
3. If `otherShot1` is not 0 and `aShotReloadRate >= otherShot1`, stop; the
   same for `otherShot2`.
4. Choose a column (from 1). If `track` is 0, the column is **find column**
   of `playerXr + 8`, but 11 if that is 12 or more. Otherwise the column is
   `columnFireTable[cfir]`, and `cfir` increases by 1.
5. Find the lowest live alien in the column: starting at index
   `column - 1`, check five data-page bytes 11 apart (indexes wrap at 256),
   taking the first that is not 0. If none is, stop.
6. With that alien's **coordinates**, set `xr` to its `xr + 7` and `yr` to its
   `yr - 10`, set bit 7 of `status`, and add 1 to `step`.

Otherwise, when `xr` matches the beam:

- If `status` bit 0 is set, it is exploding: decrement `blow`. If `blow` is
  now 3: erase shifted `pic` at the shot's position, set `pic` to
  `alienShotExplosion`, subtract 2 from `xr` and from `yr`, and draw with
  collision. If `blow` is now 0: erase shifted `pic` at the shot's position.
- Otherwise it moves: add 1 to `step`; erase shifted `pic` at its position;
  set `frame` to `(frame + 3) % 12` and `pic` to `pictures[frame:frame + 3]`;
  add `alienShotDelta` to `yr`; and draw with collision. Then, if `yr` is
  below 0x15, set `status` bit 0. Otherwise, if `collision` is set: if `yr` is
  at least 0x1E and below 0x27, the shot has hit the player, so set
  `playerAlive` to 0; and either way set `status` bit 0.

**Object 2, the rolling shot.** Set `obj2Extra` to 2. If `rolling.cfir` is 0,
set it to 0xFFFF and stop. Otherwise make `rolling` active, with
`otherShot1` `plunger.step` and `otherShot2` `squiggly.step`, and run the
active shot with `rollingShot`. If its `blow` is not 0, copy it back to
`rolling`; otherwise reset `rolling` and set `obj2Timer` to 0 and `obj2Extra`
to 2.

**Object 3, the plunger shot**, acts only when `skipPlunger` is 0 and
`shotSync` is 1. Make `plunger` active, with `otherShot1` `rolling.step` and
`otherShot2` `squiggly.step`, and run it with `plungerShot`. If its `cfir` is
16 or more, set it to 0. If its `blow` is not 0, copy it back to `plunger`.
Otherwise reset `plunger` but keep the active shot's `cfir`, set `obj3Timer`
and `obj3Extra` to 0, and, if `numAliens` is 1, set `skipPlunger` to 1.

**The squiggly shot**: make `squiggly` active, with `otherShot1`
`plunger.step` and `otherShot2` `rolling.step`, and run it with
`squigglyShot`. If its `cfir` is 21 or more, set it to 6. If its `blow` is
not 0, copy it back to `squiggly`. Otherwise reset `squiggly` but keep the
active shot's `cfir`, and set `obj4Timer` and `obj4Extra` to 0.

### Object 4: the squiggly shot or the saucer

The handler acts only when `shotSync` is 2:

1. If `saucerStart` is 0, or `squiggly.step` is not 0, run the squiggly shot
   and stop.
2. If `saucerActive` is 0: if `numAliens` is below 8, run the squiggly shot
   and stop; otherwise set `saucerActive` to 1 and draw the saucer.
3. If `saucerXr` does not match the beam, stop.
4. If `saucerHit` is not 0, run the **saucer hit sequence** and stop.
5. Add `saucerDelta` to `saucerXr` and draw the saucer. If `saucerXr` is
   below 0x28 or at least 0xE1, remove the saucer.

To **draw the saucer**, draw simple `saucerPic` at the offset of
`(0xD0, saucerXr)`. To **remove the saucer**, clear small 24 rows there, then
set `saucerStart` 0, `saucerActive` 0, `saucerHit` 0, `saucerHitTime` 0x20,
`saucerPic` `saucer`, `saucerXr` 0x29, and `saucerDelta` 2, and turn the
sound bits off with mask 0xFE.

**The saucer hit sequence**: turn the sound bits off with mask 0xFE, and
decrement `saucerHitTime`. At 0x1F, set bit 0x10 of `soundPort5`, set `port5`
to `soundPort5 & 0x30`, set `saucerPic` to `saucerExplosion`, and draw the
saucer. At 0x18, award the
saucer's score: `saucerScoreTable[sauScoreIdx]` is 0x05, 0x10, 0x15, or 0x30
for 50, 100, 150, or 300 points; set `scoreDelta` to the points and
`adjustScore` to 1, and print the matching message (`messageSaucer50` and so
on, 3 characters) at the saucer's offset. At 0, clear bit 0x10 of
`soundPort5`, set `port5` to `soundPort5 & 0x20`, and remove the saucer.

### The aliens

**Find column** of an `xr` and **find row** of a `yr` count how many 16s
bring the rack's reference coordinate up to the target, with the original's
quirk. Counting from `a` (`refAlienXr` or `refAlienYr`) to `h`: start with
`c` = 0. If `a >= h`, then repeatedly add 1 to `c` and 16 to `a` until bit 7
of `a` is clear (at least once). Then, while `a < h`, add 16 to `a` and 1 to
`c`. Find column gives the column `c` (from 1) and the column's `xr`,
`a - 16`. Find row gives the row `c - 1` (from 0) and the row's `yr`,
`a - 16`.

The **coordinates** of alien index `i` start from `refAlienYr`,
`refAlienXr`, and row 0: while `i - 11`, as a byte, has bit 7 clear, subtract
11 from `i`, add 16 to `yr`, and add 1 to the row. Then add 16 to `xr` `i`
times.

The **index** of row `r` and column `c` (from 1) is
`(rot(r) + 3r + c - 1) & 255`, where `rot(r)` rotates the byte `r` left by 3
bits.

**The alien cursor** (mid-screen interrupt): when `playerOK` is not 0 and
`waitOnDraw` is 0, find the next live alien. Starting from `alienCurIndex`,
step the index by 1. When it reaches 55, the rack moves, unless this is the
second time in this search, which ends the search with no change: set
`alienCurIndex` to 0, add `refAlienDY` to `refAlienYr` and then set
`refAlienDY` to 0, add `refAlienDX` to `refAlienXr`, toggle `alienFrame`, and
continue from index 0. An index is live when its data-page byte is exactly 1.
At the first live index: set `alienCurIndex` to it, and `alienPosYr` and
`alienPosXr` to its coordinates. If its `yr` is below 0x28, the aliens have
**invaded**. Otherwise set `alienRow` to its row and `waitOnDraw` to 1.

**Draw the alien** (end-of-screen interrupt): if `alienIsExploding` is not 0,
decrement `expAlienTimer`, and when it reaches 0, erase simple 16 rows at the
offset of `(expAlienYr, expAlienXr)`, set `shotStatus` to 4, set
`alienIsExploding` to 0, and turn the sound bits off with mask 0xF7. Otherwise, if the data-page byte at `alienCurIndex`
is not 0, draw its sprite at `(alienPosYr, alienPosXr)`; then set
`waitOnDraw` to 0. The sprite is the 16 bytes of `alienSprites` from offset
`rot(alienRow & 0xFE)`, plus 0x30 when `alienFrame` is 1.

**The saucer timer** (end-of-screen interrupt): when `refAlienYr` is below
0x78: if `tillSaucer` is 0, set it to 0x600 and `saucerStart` to 1; then
decrement `tillSaucer` (16 bits).

## Foreground

The foreground is in one state at a time. Each slot runs its state's step,
which may change the state and continue straight into the next state's
code, as described.

### Power-on and the start screen

At power-on: do the RAM reset, set `aShotReloadRate` to 8, and draw the
**status**: clear the screen; print `messageScoreHeader` at 0x241E; print
`p1Score` at 0x271C, 0 at 0x391C, and `hiScore` at 0x2F1C; print
`messageCredit` at 0x3501; and print `numCoins` at 0x3C01. Then show the
**start screen**.

The **start screen** (also after a game): set `suspendPlay` to 0 and
`numCoins` to 1; print `numCoins` at 0x3C01 and `messageCredit` at 0x3501;
clear the playfield; print `messagePush` at 0x3013 and `messageOnly1Player`
at 0x2810. The state is **waiting to start**: each step, if start is held,
begin a **new game**.

### A new game

1. Set `numCoins` to 0 and print it at 0x3C01. Set `p1Score` to 0 and print it
   at 0x271C; print 0 at 0x391C.
2. Set `suspendPlay` to 0, `gameMode` to 1, and `player1Ex` to 1, and draw the
   status.
3. Fill the shield buffer, set the reserve ships to 3 and the starting
   `refAlienDX` to 2, and clear small 32 rows at 0x391C.
4. Set the rack count to 0, every alien alive, and the starting position to
   `yr` 0x78, `xr` 0x38. Do the RAM reset.
5. **Remove a ship.**
6. Print `messagePlayPlayer1` at 0x2B11, set `isrDelay` to 0xB0, and go to
   **the prompt**, running its step now.

**The prompt**: if `isrDelay` is 0, clear the playfield and wait 25 slots
for the rack. Otherwise, if `isrDelay & 4` is not 0, clear small 32 rows at
0x271C, else print `p1Score` at 0x271C.

**Waiting `n` slots for the rack**: this slot is the first; the rack starts
in the step of the `n`-th following slot. (On the original, clearing the
playfield and redrawing take this long.) To **start the rack**: draw the
bottom line; OR the shield buffer onto the screen, for each shield `k` from
0 to 3, its 22 rows of 2 bytes into the rows from offset
`0x406 + k * 0x5A0`; set `refAlienYr` and `alienPosYr` to the starting `yr`,
`refAlienXr` and `alienPosXr` to the starting `xr`, `refAlienDX` to the
starting delta (but 2 if it is 3), and `rackDirection` to 1 if `refAlienDX`
is 0xFE and 0 otherwise; set `suspendPlay` to 1; turn on sound bit 0x20; and
go to **the game loop**, running one iteration now.

**Remove a ship**: if the reserve ships `n` are not 0, set them to `n - 1`,
**draw the reserve ships** `n - 1`, and print the digit `n` at 0x2501.

**Draw the reserve ships** `m`: draw simple the `player` sprite at 0x2701,
0x2901, and so on (512 bytes apart), `m` times; then, from the next of those
addresses, clear small 16 rows and step 512 on, stopping when the next
address would be 0x3501 (so 0x3301 is the last cleared).

### The game loop

Each iteration:

1. **Fire**: if `playerAlive` is 0xFF, `obj0Timer` is 0, and `shotStatus` is
   0: if `fireBounce` is set, clear it when fire is not held; otherwise, if
   fire is held, set `shotStatus` and `fireBounce` to 1.
2. **The player's shot hit**.
3. **The rack bump.**
4. Count the live aliens (data-page bytes 0-54 that are not 0) into
   `numAliens`.
5. If `adjustScore` is set: clear it, add `scoreDelta` to `p1Score` modulo
   10000, and print `p1Score` at 0x271C.
6. If `numAliens` is 0, the rack is cleared: skip the rest of this iteration
   and go to **the rack's end**, running its step now, in this slot.
7. Set `aShotReloadRate` from `h`, the hundreds of `p1Score`
   (`p1Score / 100`, rounded down): 0x30 if `h` is at most 2, 0x10 if at most
   10, 0x0B if at most 20, 0x08 if at most 30, and 0x07 otherwise.
8. **The extra ship**: if `player1Ex` is set and `h` is at least 15: add one
   reserve ship (`n` ships now), draw simple `player` at `0x2501 + 512n`,
   print the digit `n + 1` at 0x2501, clear `player1Ex`, set `extraHold` to
   0xFF, and turn on sound bit 0x10.
9. If `numAliens` is below 9, set `alienShotDelta` to 0xFB.
10. If `shotStatus` is not 0, turn on sound bit 0x02; otherwise turn the
    sound bits off with mask 0xFD.
11. If `playerAlive` is not 0xFF, turn on sound bit 0x04.
12. **The fleet note**: if `changeFleetSnd` is set, set `fleetSndReload` to
    `fleetDelays[j]`, for the first `j` with `numAliens >= fleetCounts[j]`;
    move the note: with `low = (soundPort5 & 0x0F) << 1`, but 1 if that is
    0x10, set `soundPort5` to `low | (soundPort5 & 0x30)`; and clear
    `changeFleetSnd`. Then decrement `extraHold`, and if it is now 0, turn
    the sound bits off with mask 0xEF.
13. If `saucerActive` is 0, turn the sound bits off with mask 0xFE; otherwise,
    if `saucerHit` is 0, turn on sound bit 0x01.

**The player's shot hit**: only when `shotStatus` is 2. Let `b` be
`shotYr`.

Wherever this sets `alienIsExploding` to 0, it then turns the sound bits off
with mask 0xF7.

1. If `b` is at least 0xD8, the shot missed: set `shotStatus` to 3 and
   `alienIsExploding` to 0, and stop.
2. If `alienIsExploding` is 0, stop.
3. If `b` is at least 0xCE, the shot hit the saucer: set `saucerHit` to 1,
   `shotStatus` to 4, and `alienIsExploding` to 0, and stop.
4. Let `b6` be `b + 6`. If `refAlienYr` is below 0x90 and at least `b6`, the
   shot hit something below the rack: set `shotStatus` to 3 and
   `alienIsExploding` to 0, and stop.
5. Find the row of `b6` and the column of `shotXr`; set `expAlienYr` to the
   row's `yr`, `expAlienXr` to the column's `xr`, and `shotStatus` to 5.
6. If the data-page byte at the row and column's index is 0, the shot hit an
   alien shot: set `shotStatus` to 3 and `alienIsExploding` to 0, and stop.
7. Otherwise set that byte to 0; turn on sound bit 0x08; set `adjustScore` to
   1 and `scoreDelta` to
   10 for row 0 or 1, 20 for row 2 or 3, and 30 for any other row; draw
   `alienExplosion` at `(expAlienYr, expAlienXr)`; and set `expAlienTimer` to
   0x10.

**The rack bump**: when `rackDirection` is 0, if any of the 23 bytes from
0x3EA4 is not 0, set `refAlienDX` to 0xFE, `rackDirection` to 1, and
`refAlienDY` to 0xF8. When `rackDirection` is 1, if any of the 23 bytes from
0x2524 is not 0, set `refAlienDX` to 3 if `numAliens` is 1 and to 2
otherwise, `rackDirection` to 0, and `refAlienDY` to 0xF8.

### The rack's end

If `playerAlive` is not 0xFF, wait until it is. Otherwise set `isrDelay` to
0x30 and wait until it is 0. Each step of that wait checks `isrDelay` first:
if it is 0 the wait is over; otherwise, if `playerAlive` is not 0xFF, the
wait becomes waiting until `playerAlive` is 0xFF, checked again at once.
Each wait checks its condition once per step, starting in this step.

Then: set `suspendPlay` to 0, clear the playfield, and do the RAM reset. Let
`n` be `(rack count & 7) + 1`; set the rack count to `n`, the starting `yr`
to `alienStartTable[n - 1]`, and the starting `xr` to 0x38. Fill the shield
buffer, set every alien alive, and wait 26 slots for the rack.

### After a death

In the slot after the interrupt the player's explosion ended in: set
`suspendPlay` to 0. If there are no reserve ships, print the digit 0 at
0x2501 and go to **game over**. Otherwise remove a ship, set `suspendPlay` to
1, turn on sound bit 0x20, and go to the game loop, running one iteration
now.

### Invasion

When the alien cursor finds the aliens have invaded, set `invaded` to 1. In
the next slot, set `playerAlive` to 0 and go to **invaded**, running its step
now. Each step of
**invaded**: run the player's shot hit and turn on sound bit 0x04; then, if
`playerAlive` is 0xFF, set `suspendPlay` to 0, draw the reserve ships 0,
print the digit 0 at 0x2501, turn the sound bits off with mask 0xFB, and go
to **game over**.

### Game over

If `p1Score` is greater than `hiScore`, set `hiScore` to it and print it at
0x2F1C. Then print `messageGameOver` from 0x2D18 one character at a time:
print a character and set `isrDelay` to 7, and print the next when a step
finds `isrDelay` is 1. The first character is printed now. When a step finds
`isrDelay` is 1 after all 10 characters, set `isrDelay` to 0x80, and when a
step finds it 0, clear the playfield, set `gameMode` to 0, set `port5` to 0,
show the start screen, and run its step now.

## Sound

The original machine has no sound chip: the program sets bits on two output
ports, `port3` and `port5`, and each bit drives a sound circuit.

**Turning on sound bit `b`** sets `soundPort3` to `soundPort3 | b`, and
**turning the sound bits off with mask `m`** sets it to `soundPort3 & m`;
both then set `port3` to `soundPort3`. (The RAM reset clears `soundPort3`
but leaves `port3`.) `port5` is set only where this specification says so.

**The fleet timer** (end-of-screen interrupt), which plays the marching
notes:

1. Decrement `fleetSndHold`; if it is now 0, set `port5` to
   `soundPort5 & 0x30`.
2. If `playerOK` is 0, set `port5` to `soundPort5 & 0x30` and stop.
3. Decrement `fleetSndCnt`. If it is not 0, stop.
4. Set `port5` to `soundPort5`. If `numAliens` is 0, set `port5` to
   `soundPort5 & 0x30` and stop.
5. Set `fleetSndCnt` to `fleetSndReload`, `changeFleetSnd` to 1, and
   `fleetSndHold` to 4.

### Sounds

Define these ten sounds before the first frame, with samples of your own
design (the original's were analog; approximate them as you like):

| Name | Port bit | What it is |
|---|---|---|
| `ufo` | `port3` 0x01 | the saucer's siren, which loops |
| `shot` | `port3` 0x02 | the player fires |
| `playerDeath` | `port3` 0x04 | the player explodes |
| `invaderHit` | `port3` 0x08 | an alien explodes |
| `extraShip` | `port3` 0x10 | the extra ship is awarded |
| `fleet1`-`fleet4` | `port5` 0x01, 0x02, 0x04, 0x08 | the four fleet notes, falling in pitch |
| `ufoHit` | `port5` 0x10 | the saucer explodes |

`port3` bit 0x20 is the amplifier: while it is clear, every bit counts as
clear. After each frame's game logic, compare each bit, as it counts, with
its value after the previous frame's (all clear before frame 0). For each bit
in the table's order, if it came on, call `arcade.loop("ufo")` for `ufo` and
`arcade.play(name)` for the others; if `ufo`'s bit went off, call
`arcade.stop("ufo")`. Make no other `play`, `loop`, or `stop` calls.
