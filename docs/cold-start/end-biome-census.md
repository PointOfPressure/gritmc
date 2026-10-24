# end-biome-census — the End per biome, and the references that were not there

Branch `end-biome-census`, off `nether-biome-census` 5f1bd4ad (it needs that
branch's `region-differ --by-biome`, which is not on main). 2026-08-18,
Threadripper, `~/rustmc-end`, worktree `~/grit/work-end`. T0 GREEN.

## The one-line version

The End's residual is **0 in all five biomes**, `the_end` had **never been
measured**, and every reference the published numbers rested on had been
**gone for eleven days**.

## What the job was

`docs/PARITY.md` §6 published two aggregate numbers for the End at r=16 and
decomposed for neither. The sister nether lane had just shown that a per-biome
census finds what an aggregate hides. It does here too, in the opposite
direction: the nether's aggregate hid a defect, the End's hid a **coverage
gap**.

## Finding 1 — every End reference was destroyed and nobody had noticed

`docs/DETERMINISM.md` §6 closes the End against six references by name:
`van-end-20260722-r16-a`, `-b`, `det-ens-end-c`, `-d`, `-e`, `-f`. A search
across every `oracle-work` on the box — ~150 lane checkouts — finds **none of
them**, under that name or any other. They went in the 2026-08-07 disk failure
with everything `PARITY.md` §5 rested on. The overworld's and the nether's were
re-harvested; the End's were not, **because a dimension reading 100.000000%
never looked like it needed anything**.

So from 2026-08-07 to 2026-08-18 the figure `ci/figures.toml` calls *"the
strongest figure the project has and the only one that needs no qualifier"*
could not be reproduced. Not a claim it was wrong — a claim nobody could have
found out.

The only survivors are `van-end-20260722-c8362` and `-b`, 2026-07-30, covering
chunks 74..92 x 53..71 (r=9 at centre 83,62). Both predate `HARVEST-ORDER` and
state no order, so no figure may rest on them.

**Fourteen order-stamped replacements harvested**, `GEN_ORDER=rows`,
`dump_order=zx`, `jvm_opts=` (default pool, so every floor here is an
UNCLAMPED floor carried with its N):

```text
  gen-wide-20260722-r16-end-{a..f}     r=16 c(64,64)   1,089 chunks
  gen-wide-20260722-r16-end-rr-{a,b}   r=16 c(64,64)   GEN_ORDER=rows_rev
  van-end-20260722-c0_0-open-{a,b,c}   r=7  c(0,0)       225 chunks
  van-end-20260722-c8362-open-{a,b,c}  r=7  c(83,62)     225 chunks
```

About a minute each. They are in `~/rustmc/oracle-work` and shared.

## Finding 2 — the published window weights `the_end` at exactly zero

```text
  r=16, centre (64,64) — 1,089 chunks, 71,368,704 blocks
    end_midlands        25,821,184   36.18%
    small_end_islands   19,464,192   27.27%
    end_highlands       18,415,616   25.80%
    end_barrens          7,667,712   10.74%
    the_end                      0    0.00%
```

The centre is block (1,024, 1,024), 1,448 from the origin, and
`TheEndBiomeSource` returns `the_end` only inside 1,000. The island with the
ten obsidian spikes, the exit portal and the arrival platform was outside every
window in the file. The r=7 window at c(83,62) is the same shape.

## What moved: all five biomes carry a figure, all five are 0

```text
  biome              window          blocks       ours   floor (N)
  the_end            c(0,0)   r=7    14,745,600      0   0   (N=3)   <-- first ever
  end_midlands       c(64,64) r=16   25,821,184      0   4   (N=6)
  small_end_islands  c(64,64) r=16   19,464,192      0   0   (N=6)
  end_highlands      c(64,64) r=16   18,415,616      0   123 (N=6)
  end_barrens        c(64,64) r=16    7,667,712      0   0   (N=6)
```

The r=16 aggregate reproduces **exactly**: 0 unmasked against a fresh
reference, 0 against the fresh six-reference set, twin mask drops 0 positions.
The **floor does not**: 252 published, **127** measured across six same-order
references, pairwise 0/127/46/46/0 against the old 0/46/81/125/171/206. The old
six recorded `harvest = "unrecorded"`; a mixed-order floor and a matched-order
one are different quantities.

## The floor is one biome's chorus plants

`end_highlands` is 25.80% of the window and carries **123 of the 127**;
`end_midlands` 4; the other two none. Every position is `chorus_plant` or
`chorus_flower` against `air`, in five chunks, y58..80. `chorus_plant` is
placed in `end_highlands` alone and grows upward across chunk boundaries off a
`MOTION_BLOCKING` heightmap, so it reads a neighbour that may not have
decorated yet — the End's `basalt_deltas`. The four `end_midlands` positions are
plants rooted in `end_highlands` whose tops cross the boundary.

## The order control, and it is the cleanest in the project

```text
  vanilla rows-a  vs vanilla rows_rev-a      944   100% end_highlands
  vanilla rows_rev-a vs rows_rev-b           317   (a rev pair is NOISIER)
  vanilla rows-a  vs rows-b                    0
  OURS (zx)       vs vanilla rows-a            0
  OURS (zx)       vs vanilla rows_rev-a      944   <-- the same 944, to the block
```

Our dump differs from a reverse-order vanilla by exactly what a forward-order
vanilla differs from it by, because our dump **is** a forward-order vanilla bit
for bit. So the End's order sensitivity is 0 against 944 where the nether r=7
window's is 335 against 7,638. `PARITY.md` §3's *"253 in the End"* is the r=7
reading and does not carry.

## What the residual is made of: nothing — so the item is coverage

The r=16 window returns `starts=0 pieces=0` and its whole census is `air`,
`end_stone`, `chorus_plant`, `chorus_flower`. **The widest End window this
project has exercises two block families.**

```text
  end_spike             the_end            c(0,0)   r=7    reached, exact
  end_platform          the_end            c(0,0)   r=7    reached, exact, NOT
                                                           statistically —
                                                           fixed_placement at
                                                           (100,49,0), chunk
                                                           (6,0), in this
                                                           window and no other
  end_island_decorated  small_end_islands  c(64,64) r=16   reached, exact
  chorus_plant          end_highlands      c(64,64) r=16   reached, exact
  end_city              end_highlands      c(83,62) r=7    reached, exact,
                                                           70 pieces, no ship
  end_gateway_return    end_highlands      NONE            NOT REACHED, unported
```

**`end_gateway_return` is in no measured window and that is arithmetic.**
`rarity_filter chance=700` then a biome filter to `end_highlands`; over all
three windows — 1,539 chunks, ~398 end_highlands chunk-equivalents — the
expected count is **0.57** against an observed 0. Censused directly: zero
`end_gateway` blocks anywhere, zero bedrock outside the ten spike tops. **This
port could place them perfectly or not at all and every End figure would read
the same.**

## Nulls — do not re-run these

* **NULL: there is no End defect to find.** Five biomes, three windows,
  fourteen references, residual 0 everywhere. Anyone sent to "improve the End's
  number" is being sent to improve a zero. The open work is harvest work.
* **NULL: do not widen the r=16 window hoping to catch a gateway.** ~1,750
  end_highlands chunks are needed, 6.2x what r=16 carries. The cheap route is a
  `starts`-guided search over candidate r=7 squares, not a bigger square.
* **NULL: do not port `end_gateway_return` first.** Until a window containing
  one exists, it would produce a feature nothing can grade. Item 1 in
  `PARITY-ROADMAP` §62 comes before item 4 for that reason.
* **NULL: the r=16 window is not a structure test and never was.**
  `starts=0 pieces=0`. Do not use it to argue anything about `end_city`.

## Open, cheap, and named in PARITY-ROADMAP §62

1. A window containing a return gateway (~1,750 end_highlands chunks).
2. Re-harvest the End at seeds 12345 and 31415926 — `PARITY.md` claimed three
   seeds until today and only 20260722 is reproducible. **Two minutes each.**
3. A window containing an end ship (all three c(83,62) references hold the same
   city, 70 pieces, 0 `brewing_stand`).

## Also fixed on this branch, and not about the End

`ci/check-cited-audits.py` went red on **every lane's remote T0** today with
four `CITATION UNREACHABLE` lines claiming two branches had been force-pushed.
Neither had: a gate directory is an rsync with no `.git`, so `git show
<sha>:<path>` fails for every row. It now probes for a repository once and
skips loudly. `ci/check-cited-audits-falsify.py` builds its own throwaway
repository so its three fault cases still run where there is none, and its
green control prints `-- NOT RUN` rather than a vacuous ok.

## New in the tree

* `ci/end-biome-window.sh` — the artefact for every End figure, per biome,
  plus the floor decomposition and the order control.
* `docs/DETERMINISM.md` §§51, 49, 50.
* `docs/PARITY-ROADMAP.md` §62.
* `ci/figures.toml` — `end-per-biome-n6`, `end-floor-highlands-n6`,
  `end-floor-midlands-n6`, `end-order-rows-vs-rowsrev-r16`.
