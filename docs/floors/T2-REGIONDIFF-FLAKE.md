# `ci/t2-regiondiff.sh` is a zero-flake gate that flakes, measured at N=6

**Measured 2026-08-18, branch `floors-n-campaign`, on the Threadripper.**

Found while gate-checking this branch, not looked for.

## What the gate is

```
# T2-regiondiff — prove the Anvil parser + differ end-to-end BEFORE rustmc
# has worldgen: generate the same seed twice with real vanilla, diff the
# inner square, demand bit-exact 100.000000%. If vanilla-vs-vanilla isn't
# 100%, the parser/differ/gen-harness is broken — fix that, never the
# tolerance (CI.md zero-flake rules).
```

Seed 20260722, r=2, centre chunk (64,64), `GEN_ORDER=square`, compared over
the inner 3x3 chunks (bbox 63 63 65 65, 884,736 blocks). It ran RED on a fresh
worktree at **99.989375%**.

## The premise is right and the gate is still flaky

The obvious reading — a gate demanding exact vanilla reproducibility while this
whole campaign measures vanilla as irreproducible — is **wrong**, and it is
worth writing down because it was my first reading.

Six unmodified-vanilla references of that window, fifteen pairs, counts from
`region-differ`'s own `mismatches=` field:

```
  a-b  94    a-c   0    a-d   0    a-e   0    a-f   0
  b-c  94    b-d  94    b-e  94    b-f  94
  c-d   0    c-e   0    c-f   0    d-e   0    d-f   0    e-f   0
```

**Ten of fifteen pairs are exactly zero.** This window really is reproducible,
and the gate's premise holds: at r=2 through a single `forceload add` of a 5x5
square, vanilla's scheduler has almost nothing to reorder, which is exactly the
difference from the r=16 row-driven harvests where the floor is 6,032..17,129.

What is not true is that it is *always* zero. **One run of six — `b` — diverged
from all five others by 94 blocks.** So the gate is a coin-flip against a
divergent draw, not a constant.

Naively, if roughly one harvest in six is a divergent variant, a two-run gate
goes red with probability about `2 * (1/6) * (5/6)` ≈ **28%**. At n=6 that
estimate is soft, but the direction is not: this is not a zero-flake gate and
its header forbids the usual response.

## What diverged, and it is the campaign's own families

The 94 blocks, `a -> b`:

| count | change |
|---:|---|
| 17 | `water[level=0]` → `seagrass` |
| 13 | **`cinnabar` → `sulfur`** |
| 12 | `seagrass` → `water[level=0]` |
| 10 | `tall_seagrass[half=upper]` → `water[level=0]` |
| 8 | `water[level=0]` → `tall_seagrass[half=upper]` |
| 7 | `water[level=0]` → `tall_seagrass[half=lower]` |
| … | `sulfur_spike` appearing where the other run has `air` |

Seagrass placement and the **`cinnabar ↔ sulfur` inversion** — the same pair
`docs/DETERMINISM.md` §21 already names, where the instrument was cleared and
the inversion shown to be ours. Here both sides are unmodified vanilla, so
these 94 positions are vanilla disagreeing with itself, in the families this
campaign finds everywhere.

## What to do about it, which is not to touch the tolerance

The header is right that relaxing to a tolerance would destroy the gate: its
whole value is that a parser or differ bug shows up as a non-zero number, and a
tolerance would swallow exactly that.

The options that keep it falsifiable, for whoever owns the gate machinery:

1. **Retry the pair on a miss.** A second independent pair costs ~2 minutes and
   a genuine parser bug fails both, while a divergent draw almost never repeats.
2. **Gate on unanimity over three runs rather than equality of two.** Three runs
   with two agreeing identifies the odd one out instead of failing the suite.
3. **Report the miss set.** 94 blocks in seagrass and `cinnabar ↔ sulfur` is a
   recognisably *vanilla-jitter* signature; a parser bug would not look like
   that. The gate currently prints the histogram and could refuse only when the
   miss set is not confined to known-jittery families.

Not done here: this is the gate/CI area and changing a gate's verdict rule from
the worldgen-parity lane would be changing somebody else's instrument on the
strength of one afternoon's n=6.
