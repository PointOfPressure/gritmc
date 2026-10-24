# COLD START — branch `gameplay-block-placement`

### COLD START — branch `gameplay-block-placement`, 2026-08-12

**The item: §21's UNIT 3 has two rows an order of magnitude below the rest —
`block` 43/1196 (3.60%) and `block_state` 87/32366 (0.27%). Decide which one is
being moved and what it would mean. `docs/PARITY-ROADMAP.md` §41.**

Remote dir `~/rustmc-place`, base `0679f236`, worktree
`/home/sayer/grit/work-place`. The analytical half booted nothing and never
took the box-wide corpus lock: it is the pinned jar's own
`generated/reports/blocks.json`, the six recordings at
`~/rustmc-cov2/oracle-work/gameplay-selfcheck/r[1-6].json`, and static Python.

### The answer in one line

**32,366 is a Cartesian product, not a work queue. 116 schemas is the unit
that is.**

```text
  block_state         87 / 32366    0.27%     <- artefact
  block_schema        31 /   116   26.72%     <- the work queue
  block_property      34 /    93   36.56%
  block_prop_value    79 /   435   18.16%
```

A factor of **99** between two readings of one registry, on one ensemble, one
jar, one corpus.

### The proof, because this one is arithmetic rather than argued

For **every one of the 1,196 blocks**, `len(states)` is exactly the product of
its property-domain sizes. **Zero exceptions.** Nothing is pruned, no state is
unreachable by construction, so

```text
  32,366 = SUM over blocks of PROD over properties of |domain|
```

and it holds no information beyond the list of distinct property-set schemas
and how many blocks share each. There are 116. One of them — walls,
`east north south up waterlogged west` — is **32 blocks x 324 states = 10,368,
which is 32.0% of the entire block-state registry**, and all 10,368 come out of
`WallBlock.makeWallState` and `WallBlock.shouldRaisePost`. `note_block`'s 1,350
are `instrument` x `note` x `powered`. Top 20 blocks are 27.0% of the
denominator; top 50 are 47.9%.

The identity is now **asserted inside `ci/vanilla-reach-census.py`**, which
refuses if a future registry breaks it, rather than being described in prose
that nobody re-checks.

### What the census actually counts as observed — its docstring was wrong

`registry_reach` opened *"What the recordings prove the corpus's worlds ever
contained"* and said absence is therefore evidence. True for `entity_type`,
`sound_event`, `block_entity_type`, `menu`. **False for `block` and
`block_state`** — the two rows a reader is most alarmed by. Both are read out
of `block_update`, `section_blocks_update` and `block_event`, which vanilla
sends when a block **changes**; a block merely present is never mentioned.

The corpus's own substrate proves it. Vanilla's superflat is bedrock, two dirt,
grass. `grass_block` scores because things get placed on it; **`dirt` and
`bedrock` score zero in all six recordings** while sitting under every rig in
the corpus. Read the row as *blocks the corpus caused vanilla to broadcast a
change of* — a set our own scenarios choose. Docstring corrected in
`68dcc5dc`.

It is **not** circular in the way the brief suspected: the recordings are of
vanilla, so the numerator is vanilla's emission, not ours. It is off in a
different direction — it prices the corpus's ambition, and no verdict anywhere
depends on it.

### What was killed as a null — do not re-run these

* **The 1,196-block placement sweep.** It works and it was refused on purpose.
  `ci/block-items-golden.sh` counts 1,054 items that place a block, so a sweep
  takes `block` from 43/1196 to about 1054/1196 — **3.60% to ~88%** — with
  every check passing or failing for the same reason. What 88% would then mean
  is "the corpus emitted a `use_item_on` naming each of 1,054 items and vanilla
  answered". Not one additional mechanism. §41.1.
* **Quoting `0.27%` as coverage.** It is quoting multiplication. If you need
  the state row, print `block_schema` beside it — the census now does, and the
  `SENSITIVITY` spread excludes `block_state` and says so on the line (it was
  inflating that spread from 55x to 300x for free).
* **Taking `shelf` or `hanging sign` next for their state counts.** 768 states
  each, but both are one `facing`/`rotation` schema with a waterlog factor
  bolted on, both already covered in kind by the sign checks, and taking them
  moves `block_state` a long way while moving `block_schema` by two. That
  temptation is the entire subject of §41.
* **`ci/t2-gameplay-selfcheck.sh` as the stability instrument for three new
  files.** 12 cold boots of a 96-scenario corpus is ~3h holding a lock five
  branches and a gate chain are queued on. `ci/place-subset-probe.sh` asks the
  same question of the three scenarios alone, in about a minute a run, outside
  the lock — and states plainly that it is not a baseline.

### The trap that cost the first two hours, and it is not in AGENT-BRIEF

**`ci/gen-gameplay-tables.sh` over a bare `ssh` dies at `cargo: command not
found` and takes the rest of the table set with it.** The AGENT-BRIEF's setup
block runs it without `export PATH=$HOME/.cargo/bin:$PATH` — the inner-loop
block two paragraphs later has the export, the setup block does not. It failed
at `ci/dump-recipe-unlocks.sh`, so every table alphabetically after it was
never regenerated, and `data/vanilla/use_without_item.json` stayed the `cp -rn`
copy from `~/rustmc`. T0 then came back **RED on
`ci/check-use-probe-falsify.sh`: "the dumper swept 1 stances,
ci/checks/use-probe-dependent.toml pins 12"**, which reads exactly like a
half-landed `golden-probe-audit` and is not: the table simply had no
`probeStances` key because the dumper that emits it never ran. If you see that
red, re-run the generator with cargo on PATH before you believe anything about
the branch.

Worse, the setup command as written **exits 0 anyway** if you close it with an
`echo`, so the failure is silent. Check `GEN_EXIT` and check for
`gen-gameplay-tables: ... FAILED` in the log.

### What was added

`ci/checks/gameplay/730`, `731`, `732` — range **730-739** taken; `780` and
`790` were already allocated and `790` is still the highest-numbered file.

```text
  730  a wall between two walls connects and drops its post
  731  a block on top of a wall raises its connection to tall
  732  a candle placed on a candle makes it two candles
```

`730`/`731` are a pair and the pair is the point: `makeWallState` returns
`NONE`/`LOW`/`TALL` and every connected side `730` produces is `LOW`, so a
server on the fence model with `LOW` as the constant for "connected" passes
`730` for ever. `731` puts a solid block over one wall of a connected pair —
the `isCovered` arm — and the *neighbour's* side must not move, because its own
`above` is still air. `732` is the corpus's only placement that reads the
target block's state instead of replacing it (`CandleBlock.canBeReplaced`,
three guards, all cleared), and a server doing the natural
resolve-face-offset-write puts the second candle a block higher — the two
answers differ in position as well as state.

**Corpus 93 -> 96. Quote both bases.** A figure taken before this branch is out
of 93 and after is out of 96; §20 exists because a comparison across bases was
made once already.

### The movement, and it is the argument rather than a footnote

```text
  unit                  before          after       delta    percent
  block               43 / 1196     45 / 1196        +2     3.60% ->  3.76%
  block_state         87 / 32366    92 / 32366       +5     0.27% ->  0.28%
  block_schema        31 / 116      33 / 116         +2    26.72% -> 28.45%
  block_property      34 / 93       35 / 93          +1    36.56% -> 37.63%
  block_prop_value    79 / 435      84 / 435         +5    18.16% -> 19.31%
```

**Identical work, a factor of 170 in apparent value between the artefact unit
and the work-queue one.** The two schemas gained hold **10,640 of the 32,366
states between them — 32.9% of the whole denominator — and this branch observed
5 of those 10,640**, with both mechanisms now under a check. If you take one
thing from this branch, take that sentence.

Vanilla's own answers, so nobody re-runs them: `730` is one
`section_blocks_update` of three walls with the middle at
`east=low,west=low,up=false` and both ends at `up=true`; `731` is
`east=tall` on the clicked wall with the neighbour absent from the packet;
`732` is `candles=2` at `[1,0,1]` **and `minecraft:air` at `[1,1,1]`** — vanilla
putting the refutation of the resolve-face-offset-write model on the wire.

### The single next item

**`MULTIFACE` — `glow_lichen` and its two siblings, 3 blocks x 128 states,
schema `down east north south up west waterlogged`.** It is not the largest
unobserved schema and it is the one worth taking: clicking an existing lichen
on a second face **adds a face to the state already there** rather than placing
a block, and it is the only block class in the registry that holds six
independent faces at one position. Nothing in the corpus has that shape — `732`
is the closest and it accumulates a scalar, not a set.

Closing result: one scenario in `733`-`739`, a baseline row, and
`block_schema` at 34/116 with the multiface schema named in the census's
observed list. Price it against a ceiling first — the corpus substrate is a
superflat with no ceiling, so the rig has to build its own, and that is the
same objection that killed cave vines for `gameplay-block-classes`.
