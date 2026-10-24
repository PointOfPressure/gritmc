# Is a floor conditional on the box being quiet? No.

**Measured 2026-08-18 on the Threadripper, branch `floors-n-campaign`.**

Every floor this project has published was harvested on an idle box at ~70 s a
reference. The N campaign for `docs/PARITY.md` §2 had to be run while three
other lanes were recording gameplay corpora, at load average 36-100, and
references took **86-200 s** instead of 70 — up to 2.9x. A floor is a
measurement of vanilla's own scheduler jitter, so the worry writes itself: if
it is really a measurement of the *box's* jitter, then every floor in the
ledger is conditional on a condition nobody recorded, and none of them is
comparable to one taken today.

It is not. The floor does not move.

## The control

Three more references of the seed-12345 r=16 overworld window — the one window
with forty quiet references already on disk — harvested under load, changing
nothing else. Same seed, same radius, same centre, same `GEN_ORDER=rows`, same
`server-26.2.jar`, same generator script.

| reference | load at start | wall time |
|---|---:|---:|
| `floors-ow12345r16-load-la` | 99.88 | 200 s |
| `floors-ow12345r16-load-lb` | 83.94 | 104 s |
| `floors-ow12345r16-load-lc` | 50.02 |  86 s |

against `gen-wide-12345-r16-ow-{c,d,e}`, harvested 2026-08-10 on a quiet box at
~57-70 s.

## The reading

Pairwise width |A != B| over the r=16 window, bbox 48 48 80 80, full height:

| group | pairs | min | median | max |
|---|---:|---:|---:|---:|
| loaded-loaded | 3 | 15,235 | **15,820** | 16,888 |
| quiet-quiet | 3 | 16,320 | **16,712** | 17,779 |
| cross | 9 | 13,204 | **15,351** | 16,851 |

The three distributions overlap almost completely. The loaded group's median is
**5.3% BELOW** the quiet group's, which is the wrong sign for the hypothesis and
well inside the spread the quiet references show among themselves — 13,317 to
17,779 over the sixteen `rows` references on the box, a 34% range with no load
variation in it at all.

Both floors read on the mixed six-reference set reproduce the quiet-only set:

| | quiet c..h | mixed la,lb,lc + c,d,e |
|---|---|---|
| pairwise median | 16,270 | 15,932 |
| `innocent_5` | 2,577..4,835 | 2,829..4,247 |
| `union_5` | 37,248 | 36,773 |

## What this licenses, and what it does not

**Licensed:** floors may be harvested on a contended box. A 2.9x change in
wall-clock harvest time buys no detectable change in the floor, so the N
campaign's references are comparable to the 2026-08-10 ones and to each other,
and there is no need to wait for a quiet machine to buy resolving power.

**Not licensed:** this says nothing about *harvest order*, which moves the same
window by 150,000 blocks (`docs/DETERMINISM.md` §15) and remains the condition
that actually matters. It also says nothing about a box loaded in a *different*
way — every lane running here was a JVM corpus recording, and all this shows is
that vanilla's worldgen scheduling is insensitive to that particular
competition. The mechanism is consistent with it: `Util.makeExecutor` passes
`maxAllowedExecutorThreads()` as a `ForkJoinPool` **parallelism** rather than a
thread cap, so the pool spawns compensation workers regardless of what else the
box is doing, and the jitter is internal to the pool rather than a product of
how much CPU it has to itself.

n=3 loaded references. The conclusion is a negative one and a negative at n=3
is weaker than a positive would be; it is stated as "no detectable effect at
this n", not as "no effect".
