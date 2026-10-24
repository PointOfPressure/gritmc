# Cold start — branch `commands-surface`

---

### COLD START — branch `commands-surface` off `f82b2eb` (main at 2182), 2026-08-18

**The command surface is now generated from the jar, the corpus can see the
command tree for the first time, and the worklist is 10 roots rather than 90.**
Static gates green: `rustmc-server` 590 passed, `oracle-runner` 116 passed,
`ci/command-surface.py --check` matches the jar, the two falsifiers report 8
and 11 cases with their green controls holding, and
`ci/decoded-kind-reach.py` ratchets at 8 dead kinds. Not merged. Worktree `~/grit/work-cmd`, remote dir `~/rustmc-cmd`.
**No corpus recording was taken.** One was queued and then dropped on the
owner's instruction, to give the box back its CPU; the lock was released and
nothing here holds a place in it. Everything below is derived from instruments
that need no recording — the pinned jar, a real dedicated dispatcher, and the
server's own tree — which is why it reads without one.

The single thing that recording would have done is flip
`ci/checks/command-tree-baseline.json` from `provenance: "derived"` to
`"recorded"`. Until then a disagreement there is **reported and not fatal**, so
this branch cannot redden another lane's run.

#### The three numbers, and none of them is 92

`generated/reports/commands.json` has 92 roots and that is the wrong number
twice.

* **90** is what a *dedicated* server registers. `CommandsReport` builds with
  `CommandSelection.ALL`; `Commands.<init>` gates two classes on
  `includeIntegrated`, so `/publish` and `/unpublish` are not there.
* **10** is what a *non-operator* is told about, after `fillUsableCommands`:
  `me help list msg tell w random teammsg tm trigger`. The corpus's players
  are not operators, so the other 80 are roots no scenario can invoke or even
  see.
* **1** is how many of those ten we have: `list`.

`docs/generated/command-surface.tsv` carries all of it with per-root cost, and
`ci/command-surface.py --check` refuses when it drifts. **There is no
hand-written command table in this repo and there must not be one.**

#### What to run

    ci/command-surface.py --write      regenerate the surface
    ci/command-surface.py --check      refuse on drift (in ci/t2-gameplay.sh)
    ci/command-surface.py --worklist   unported roots with their cost
    ci/command-surface-dump.sh         vanilla's executed dedicated dispatcher
    ci/command-tree-parity.py          the ratchet (in ci/t2-gameplay.sh)
    ci/command-tree-parity-falsify.py  its three refusals (in ci/t0.sh)

#### Two derivations, because two static readings prove nothing

The dedicated/integrated split is read out of `Commands.<init>`'s bytecode,
structurally rather than by offset. That is still static, so
`ci/command-surface-dump.sh` builds a real
`new Commands(CommandSelection.DEDICATED, ctx)` and reports what came out.
They agree at 90 and `--check` refuses when they do not.

The dumper **never names an individual command**, which is the difference
between it and `ci/commands-golden.sh`. That one registers exactly what rustmc
implements, so it cannot name a command we have not thought of; asking it for
the surface is asking our own registry and getting our own answer back.

#### The corpus had never looked at the command registry

`ClientboundCommandsPacket` had no decoder arm. Adding one was not enough:
`record::record` logs both players in and settles them **before** the first
measured window, so a scenario naming `commands` in `observe` would record
nothing and pass by being empty. It is captured in `Session::liveness` — the
funnel every drain routes through, which matters because `sendCommands` runs
*after* the teleport in `placeNewPlayer`, so the join loop returns first — and
reported per scenario like `anchor` and `spawn`.

The decoder reads the roots and stops. `enumerateNodes` numbers the root's
children `1..=count` contiguously and every root command is a literal, so they
are reachable without decoding one argument. Everything past them stays in
`rest` and is still compared byte for byte.

It is tested against vanilla's real 265-byte, 10-root packet. **Deleting the
redirect-index read makes it find five roots instead of ten**, and no test
built on our own two-root, redirect-free tree could have caught that.

#### The baseline is a prediction, and the first real run judges it

`ci/checks/command-tree-baseline.json` has `provenance: "derived"`: vanilla 10,
rustmc 1, predicted from the offline instruments because the lock was six
deep. A disagreement is reported in full but does **not** fail the gate while
provenance is `derived`.

**This is the first thing for the next agent to close.** Any corpus run
exercises it. If it agrees, re-run

    ci/command-tree-parity.py WORK/vanilla.json WORK/rustmc.json --write-baseline

to flip provenance to `recorded` and make every arm fatal. If it disagrees,
that is a finding and three instruments are in play: the executed dispatcher,
the static jar reading, and our own tree.

#### Two follow-ups this branch leaves deliberately open

**Wire `ci/decoded-kind-reach.py` into `ci/t2-gameplay.sh`.** It is committed,
baselined and validated, and it is *not* wired: a t2 run was in flight and bash
reads a script incrementally, so editing one under a running gate is its own
defect. One block beside the command-tree gate, after the compare.

**Flip the command-tree baseline to `recorded`.** See above; any corpus run
does it.

#### Where the work is, and the two ends of it

`docs/PARITY-ROADMAP.md` §82. The header line that settles the ordering is

    level0 and shape=literals and not ported: (none)

Every one of the nine remaining reachable roots needs an argument node or a
redirect, and `commandtree.rs` refuses both **by type** on purpose. So each is
a type-system job first, and relaxing that refusal in passing re-introduces
the partial implementation that reads complete from the call site.

That is scoped to level 0, and §82.8 corrects an over-generalisation of it.
Ten roots an *operator* sees are pure literals and portable with today's
`Node`, gated at op levels by `ci/commands-golden.sh`:

    banlist difficulty jfr perf reload save-all save-off save-on stop version

None of them moves any scenario or the ratchet, because the corpus has no
operators. Each buys a byte-exact tree and nothing observable. `/version` is
the worked example of what they still cost: its reply is nine lines vanilla
hands over via the public `dumpVersion`, and **one of them is
`Date.toString()` in the server's zone**, so the cheapest-looking reply in the
game is locale-coupled. Do not treat that list as an afternoon each.

* **Cheapest *tree*: `/me`** — but not the cheapest command; see §82.7 before
  starting it. Its reply is a `player_chat` bound to `minecraft:emote_command`
  and **nothing in this repo can produce vanilla's bytes for it offline**, so
  capture them first and do not register `/me` before its reply exists. One
  argument, parser `message`, which
  vanilla writes as a `SingletonArgumentInfo` — confirmed against real bytes,
  where the node reads flags `06`, no children, name `action`, parser id 20,
  then *nothing*. One parser, not fifty. `teammsg` is the same shape.
* **Highest leverage: the unknown-command reply.** §82.6. We answer any
  command a player may not use with silence; vanilla sends
  `command.unknown.command` plus a styled `command.context.here`. That one
  component pair is the observable answer for all 89 unported roots and every
  typo, and it needs neither argument nodes nor redirects. It is still the
  expensive item — the styled context must go byte-exact through `rest` — and
  **the silence is deliberate**, not an omission. Get vanilla's own bytes out
  of the jar before writing a line of it, the way `ci/commands-golden.sh` does
  for the tree.

`/seed` shows the divergence without needing an unported command: it is ours,
it requires GAMEMASTERS, and the corpus's players hold level 0.

#### The blindness was not unique to commands

§82.9. `DECODED_KINDS` is 37 and a real 148-scenario recording reaches 29.
`ci/decoded-kind-reach.py` names the eight. It does **not** classify them, and
§82.9 is explicit that only `commands` is proven *drained* — proven by having
been fixed. The rest are hypotheses from reading what sends each packet, and an
earlier draft of §82.9 got `set_health` and `set_experience` wrong by filing
them as drained when the corpus is creative and peaceful, so nothing provokes
them at all. Settling one for real means the move that settled `commands`:
capture at join and see whether anything is there.

`damage_event` and `set_health` being named by `550`/`551` and absent from the
reference is the script's loudest output and is **already documented** — the
`combat` row of §4, §52's audit item 2, and the Tier 4 row of the coverage
audit, which has the stronger denominator (0 times in 17 recordings) and the
explanation: a bare-stick attack deals no damage, and creative players are
invulnerable. §52.7 lists what would settle it. This branch re-derived it; do
not read it here as new.

What is new is that the fact is now *derived* rather than written down in three
documents.

#### A finding for whoever next touches a golden dumper

§82.10. Twenty `ci/*golden*.sh` scripts guard their artefact with "run twice,
`cmp`, refuse if they differ". **Both runs share an environment**, so that
catches a clock and a random seed and is blind to a timezone, a locale, a
charset or a hostname — anything ambient that is constant within a machine,
which is exactly the kind that survives into a committed golden and then
disagrees with another worktree.

`ci/commands-golden.sh` is the measured case and now pins `TZ=UTC LANG=C` with
an `Asia/Tokyo` control. The other nineteen are **unmeasured** and this branch
does not claim they are coupled; checking one is two runs and a `cmp`.

#### Two traps this area actually sprang

**A closed loop.** `ci/commands-golden.sh` is an excellent byte gate and is
structurally blind to the surface question. Every instrument added here was
checked for whether it could name something we had not thought of.

**Reading Rust with a regex.** The generator's first run reported `uuids` — a
*child* of `list` — as a root we register and vanilla does not. That refusal
is the argument for the whole approach. Our side is now checked from the Rust
direction too, by column name rather than position, after adding the cost
columns silently moved `ported` one field right and failed the test for a
reason unrelated to its claim.
