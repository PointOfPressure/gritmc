# Cold starts — branch `corpus-tick-sections`

### COLD START — branch `corpus-tick-sections`, 2026-08-12

**The item.** `docs/PARITY-ROADMAP.md` §21's UNIT 2 classified 40 of vanilla's
120 per-tick profiler sections and left 79 `unclassified`; §26 made it 80 by
retiring one negative. §21.4 says plainly that `unclassified` means *no
information at all*, so 80 unknowns sat under the census's "34.17% provably
never run" headline. The item was to classify them into named buckets, each
with a falsifier, and to leave the genuinely undecidable ones undecided.

**What it cost:** nothing on the box. Static, over the pinned
`server-26.2.jar` and the six recordings already on disk. **No vanilla booted,
no corpus lock taken**, per the pattern §21 set — a fifth concurrent corpus run
would have slowed the gate chain for everybody. Runtime is 5 s for the
classification, 25 s for `--per-run`.

**What moved.**

```text
  before (§21 UNIT 2, re-run)   7 off / 33 unpopulated / 80 unclassified
                                40/120 = 33.33% "provably never run"

  after (ci/tick-section-classify.py)
                               63 RUNS / 7 PERIODIC / 1 off / 33 unpopulated
                                8 undecided-with-a-named-gate / 8 no-chain
                               70/120 = 58.33% provably ENTERED
                               34/120 = 28.33% provably NOT entered
                               16/120 = 13.33% undecided
```

**The correction, and it is the valuable part.** Five of the seven sections
§21.2 counts as shut by a game rule are **entered on every tick**:
`naturalSpawnCount`, `filteringSpawningChunks`, `shuffleSpawningChunks`,
`tickSpawningChunks`, `weather`. Only `customSpawners` survives; `spawner` is
undecided. The mechanism is that **vanilla pushes the profiler section and then
tests** — `tickChunks` opens with `push("naturalSpawnCount")` and reads
`GameRules.SPAWN_MOBS` forty bytes later. §21's `rule_is_gated` is a
neighbourhood check and its own docstring calls it "a staleness guard, not a
data-flow proof"; it verified a true fact (the section's code reads the rule)
that does not support the claim built on it. **UNIT 2's negatives are claims
about WORK attached to a denominator counting SECTIONS.** Both axes are now
stated separately in §36; §21.2's spawning conclusions are untouched on the
work axis.

**The 33 stand, but their stated premise does not.** §21.2 justifies them with
"`add_entity` is recorded for every entity that enters view". Measured: 960
distinct (recording, scenario, observer, raw id) pairs, **864 with no
`add_entity` in their own scenario** — 819 are the two harness players, 45 are
an item or pig alive from a scenario one or two earlier, and **0 are orphans**.
So the conclusion is sound and the sentence is not: the claim to make is "no
entity type outside the four appears in the per-corpus union".

**What is new in kind: the first positive UNIT 2 has produced.** Every tier the
census had was a negative. This is a control-flow *dominance* analysis — a push
that post-dominates its method's entry cannot be skipped — chained from
`tickServer` through call sites that are themselves unskippable. Seventeen of
the 63 come from walks rooted at the entity classes the recordings show
ticking, which is the exact mirror of the 33's argument.

**Nulls — do not re-run these.**

* **Do not rebuild §21.3's reachability instrument.** It is not merely weak, it
  is answering the wrong question; "might run" over an over-approximated graph
  cannot become "runs" by any amount of work. Dominance is the other question.
* **Do not look for a section that is unconditional in the absolute sense.**
  There is none. `MinecraftServer.tickServer` returns early when the server is
  paused for emptiness, so *every* chain in the tick carries at least that
  premise. This was a full turn's confusion: the first run reported 0 strict
  and 34 loop-premised sections and both numbers were artefacts.
* **`ci/jarcallgraph.py` must not be edited to keep code bytes.** Its digest is
  in the census stamp, so an edit silently re-stamps §21's published figures.
  `ci/tick-section-classify.py` re-reads the classes it needs and imports
  `ILEN`/`_cp` so a decode difference between the two is impossible. The stamp
  after this branch is still `census=cca2bab79417`.
* **`falling_block` cannot discharge the liveness premise.** Six runs, 12
  `move_entity_heartbeat` observations, and never one position or velocity
  update. Anything reachable only through it stays undecided.

**Two instrument bugs, both now controls in `--selftest` (5/5).** Block
successors taken from every instruction rather than each block's last gave
every block a self-edge, which made the loop relaxation delete every real edge
and called `dragonFight` unconditional when an `ifnull` guards it. And a block
that cannot reach a `return` kept the post-dominator iteration's initialiser,
putting a spurious gate on a push sitting at its method's second instruction.
Registered in `ci/t0.sh`'s explicit self-test list — the list is deliberately
not a glob, so it is a step somebody has to remember.

**No floor, and that is stated rather than omitted.** A census is not a
comparison, so ORACLE rule 6 has no premise to fire on. `--per-run` stands in
its place: r1..r6 each give 63 / 7 / 1 / 33 / 16, **spread zero on every
bucket**. Quote it beside any figure from §36.

**The single next item.** §21.5's work order needs re-reading on the entry
axis. Rows 6-8 were ranked cheapest "because they are ours", and five of their
eight sections are already entered — so what those rows cost is not reachability
but a scenario that makes the *work* observable, which is precisely what §26's
per-scenario `[[gamerule]]` mechanism was built for. The result that closes it:
a scenario setting `spawn_mobs true` for its own window, with the corpus floor
unmoved.
