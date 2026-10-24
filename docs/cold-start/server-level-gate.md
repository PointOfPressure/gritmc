# Cold start — branch `srvlevel-gate`

### COLD START — branch `srvlevel-gate` off `main 2183`, 2026-08-18

What to run, and in what order.

```text
export PATH=$HOME/.cargo/bin:$PATH   # or ci/gen-gameplay-tables.sh dies 12 generators in
ci/server-level-gates.py             # the 8 gates and what each one hides  (~8s)
ci/server-level-gates.py --emit      # data/vanilla/server_level_gates.json, for the dumpers
ci/use-golden.sh                     # regenerate use_without_item.json     (~90s, jar)
ci/placed-golden.sh                  # regenerate set_placed_by.json        (~35s, jar)
ci/server-level-gates.py --check     # refuse if a gate, a population or a stamped column moved
ci/server-level-gates-falsify.py     # 23 arms on shadow trees              (~3 min, jar)
ci/probe-absences.py --check         # now says `0 are the probe's environment`
```

Both generators write into `data/vanilla/`, which is **gitignored and
generated**. Regenerate before you measure and never read another tree's copy
for a number about yours — `docs/cold-start/probe-answering-environment.md` has
the two provisioning traps and they are both still live.

`ci/use-golden.sh` and `ci/placed-golden.sh` now run
`ci/server-level-gates.py --emit` themselves, so there is no separate step and a
stale ledger stops the pipeline before a dumper writes a table with a wrong gate
column.

---

## The one-paragraph version

§80 measured the population — 8 probe rules gate on `instanceof ServerLevel`
over 82 registry blocks, 7 in silence — and reported it rather than refusing it,
because six of the eight have rows and refusing the gate deletes every chest,
barrel and shulker box row over a branch this project may never take. That was
right and it stays right. **This branch put the gate in the row instead.** A
`gated` column names the fields of that very cell the branch decided rather than
the block: 404 live cells over 34 blocks. The verdicts are not uniform — four
container rules are reachable today, two need a subsystem that does not exist,
one is already announced — and the last `environment` absence closed on the way
past, taking that count from 3 to **0** without porting anything.

## Why the fact comes from outside the run

`instanceof` is answered by the JVM against the object's class. A `Level`
subclass cannot observe the question, cannot refuse it and cannot log it. There
is no hook, and `SignBlock` is the only exception in the registry — vanilla
hands `Util.pauseInIde` an exception, `SharedConstants.IS_RUNNING_IN_IDE` is a
non-final `public static boolean`, and `Util.setPause` is public, which is the
entire reason its 1,344 cells are refusals and not rows.

So `ci/server-level-gates.py` derives it from the bytecode and hands it to the
dumpers through `-Drustmc.gates`. The dumpers supply the half only they have:
the real `BlockState`. Two of the eight rules reach the gate only for some
states — `age>1` on the berry bush, `charges>0` on the respawn anchor — and no
bytecode walk settles that.

## The derivation is a CFG walk, and an offset range is the trap

`hides` is the invocations reachable from the gate branch's **fall-through**
minus those reachable from its **target**. The subtraction cancels the join
without special-casing it.

`SweetBerryBushBlock` is why a range does not work: its `super.useWithoutItem`
sits at a *higher* offset than the branch target while having nothing to do with
the gate. And `RespawnAnchorBlock` and `SignBlock` use the inverted
`ifeq FALSE; ...; goto AFTER` idiom, which puts the gated code *after* the
target. No single range reads both.

**Three things that cost real time and are worth not rediscovering:**

* `javap` prints `static {};` with **no parentheses**. Matching members on `(`
  leaves `<clinit>` attributed to the previous method, which put `Block.column`
  and `simpleCodec` — both field initialisers — inside
  `SweetBerryBushBlock.useWithoutItem`. Members are at exactly two-space indent;
  match on that.
* A gated `invokedynamic` means a lambda runs past the gate. The lambda bodies
  are unioned in, and the vocabulary **refuses** an unmapped call rather than
  dropping it.
* Matching the `instanceof` *operand* by string finds the same 8 rules — there
  are no descriptor-only mentions in 26.2. What carries weight is which class
  the `instanceof` names: taking **any** `instanceof` admits 14 rules that are
  not gates, because every one of these bodies also tests
  `instanceof SomeBlockEntity`.

## The column, and why it is not `reads`

`reads` says *measured, and the answer turns on something this world is empty
of*. `gated` says the fourth thing: *not measured at all, and here is which
field to disbelieve.* Its vocabulary is the column names of the table itself —
`opens`, `writes`, `sound`, `random`, `schedulesTick`, `blockEntityWritten` —
because the only question a consumer has is which field to stop trusting.

`minecraft:ender_chest` is the row to read first:

```json
"reads": ["air-world"], "gated": ["opens"]
```

One cell, two different limitations of the same harness. It gained `reads` in
§80 for the `isRedstoneConductor` check *above* the gate and could not gain
`opens` for the `openMenu` *below* it, and the only difference was that a column
existed for the first.

**A refused cell is never stamped.** A refusal already says "not measured" and
says it louder, which is why `SignBlock`'s `live` count is 0 while its row still
carries the verdict.

## What each gate hides, and whether this project can take the branch

```text
  reachable        ChestBlock(10) ShulkerBoxBlock(17) BarrelBlock(1)
                   EnderChestBlock(1)   -> false in `opens`; menu.rs implements
                                           ServerPlayer.openMenu, containers.rs
                                           implements ContainerOpenersCounter
  reachable        SweetBerryBushBlock(1) -> false in `writes`, `sound`, `random`
  needs-subsystem  RespawnAnchorBlock(1)  -> no respawn position, no explosion
  needs-subsystem  CommandBlock(3)        -> no gamerule store, no command block
                                            entities
  announced        SignBlock(48)          -> already refused, 1,344 cells
```

**`SweetBerryBushBlock` is a live bug in this server, not a gap in a table.**
For `age > 1` vanilla drops berries, plays a sound and writes `age=1` — all
inside the gate, with the `return SUCCESS` outside it. `crate::useblock` filters
on `consumes == true` and applies `writes`, so the server eats the right-click
and leaves the bush at `age=3`. Picking berries does nothing. **Two cells, and
it is the only one of the eight whose false column this server actually reads**:
`useblock` consumes `refused`, `from`, `consumes` and `writes` and nothing else,
so the 362 `opens` cells change no behaviour today. If you fix one thing from
this branch, fix that one.

## `environment` is 0, and nothing was ported for the last step

`CommandBlock.setPlacedBy` is the whole body inside the gate, and `setPlacedBy`
returns `void` — so unlike the seven `useWithoutItem` rules there was no
`consumes` to be right by coincidence about and the silence was total: three
registry blocks with no row at all. They have 3 rows and 36 cells now, carrying
`gated: ["blockEntityWritten", "writes"]`, which is the same shape as the
`probeOnly` and `faceOnly` rows §80 added.

```text
  100 blocks   morning of 2026-08-18
    3 blocks   after probe-answering-environment
    0 blocks   now
```

The rule is exactly as unreachable as it was. **The debt moved to a ledger that
can hold it** — `ci/checks/server-level-gates.toml`, verdict `needs-subsystem` —
because `environment` counts absences the table is wrong about, and this is no
longer an absence. The entry was retired because `--check` said STALE on it.

## What holds it honest

`--check` compares the bytecode walk to what the dumpers actually stamped: two
independent routes to one number, one through `javap` and one through a run of
the real rule against a real `BlockState`. Everything else in this area compares
a jar to a ledger, and two files can agree with each other while the table says
something else entirely.

Both dumpers **refuse to write a table they did not stamp**. Every piece of this
can be present and inert — the file loads, the class chain matches nothing,
`reaches` is never called, every row says what it said before — which is the
shape of the `blockEntityAsked` field that was set on every ask and read by
nobody from March to August.

`ci/server-level-gates-falsify.py` has 23 arms and six mutate the **check**
rather than the ledger, because `hides` and `columns` are derived and only
acknowledged in the ledger, so a ledger arm cannot prove the derivation works.
**Two arms were written, run, and did not fire, and are recorded as measurements
instead of shipped as arms that assert nothing** — the lambda union changes no
rule today, and the string-versus-opcode guard finds the same 8. Both are kept:
right in direction, and cheap. They are just not what is doing the work.

## What is still silent

```text
  1  StructureBlock     out-of-scope   sets the block entity's author, which no
                                       column here records
  1  JukeboxBlock       out-of-scope   needs a placement input this table does
                                       not sweep
  1  PitcherCropBlock   no-op          `0: return`
```

Three overrides, three blocks, none of them `environment`, all re-verdicted on
measurements by §80 rather than on a re-reading.

**And one population this branch did not sweep — but did measure.** Run

```text
ci/server-level-gates.py --census        # ~8s, no table needed
```

`useItemOn` is the other half of the right-click. Its table is the optional
fifth population, nothing under `crates/` opens it and its dumper is ~25
minutes, so wiring it into `PROBES` would make `--check` refuse in every tree
that has not built it. The **gates are in the jar regardless**, so `--census`
derives them and asserts nothing:

```text
  7 gated rules over 56 registry blocks, ALL SEVEN SILENT
      SignBlock 48, BeehiveBlock 2, CampfireBlock 2,
      PumpkinBlock 1, DecoratedPotBlock 1, TntBlock 1, VaultBlock 1
```

Two things to know before pricing it. **`SignBlock.useItemOn` does not
announce** — only `useWithoutItem` calls `Util.pauseInIde` — so "SignBlock is
the one vanilla tells you about" is true of one method, not of the class, and
the sign's other 48 rows are as silent as the chests. And **`PumpkinBlock` is a
second `block-state` case**, the same shape as the berry bush: shearing a
pumpkin writes `carved_pumpkin` inside the gate.

`--census` also lists the **35 calls over 7 classes with no token in
`EFFECTS`**. That is the closed vocabulary working: it was closed against the
two swept probes, and that list is the work to do before `useItemOn` can join
them — one line in `PROBES`, the tokens, and the dumper run.

**Done, on branch `useitemon-gates`; see
`docs/cold-start/useitemon-server-level-gate.md`.** Three corrections to what
is written above. The dumper is **3m48s** for the whole script — two runs of
105 million probes each and three oracles — and not 25 minutes; that estimate
is the reason this population was left optional. `PROBES` needed more than one line: a column belongs to a table rather than
to a token, and a `use_item_on.json` cell is a **(state, item)** pair, so the
guard grew an item axis. And one gate is not in the rule at all —
`TntBlock.prime` is itself `instanceof ServerLevel`, so the `setBlock(AIR)`
that the caller runs on its return value is invisible to a CFG walk of one
body, which is the same lesson as the offset range one level further out.

The other thing left alone is the **164 discovery-missed pairs** in
`docs/SERVER.md`, which need a discovery-breadth change and a new cross product.
Not cheap, and unrelated to this gate.
