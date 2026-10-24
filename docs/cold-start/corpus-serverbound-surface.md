# Cold start — branch `corpus-serverbound-surface`

This branch's own append-only history. A new entry goes on the end; nothing
above is edited.

---

### COLD START — branch `corpus-serverbound-surface`, off `origin/main` (`0679f236`), 2026-08-12

**Worktree `~/grit/work-sbound`, remote dir `~/rustmc-sbound`. No vanilla was
booted and the box-wide corpus lock was never taken** — the whole item is
static, over the pinned jar and this tree, following §21's pattern.

#### What was measured

`ci/serverbound-surface.py`, new, gated in `ci/t0.sh`:

```text
   69  serverbound play packet types    (GameProtocols, server-26.2.jar)
   27  named in rustmc-proto
   20  the harness can send             (ids::play::sb in client.rs)
   19  at least one scenario sends      (14 by a scenario's steps,
                                          5 by the connection machinery)
   20  the server has a dispatch arm for
          16  HANDLED / 1 ACCEPTED / 3 DISCARDED
   14  handled AND sent
   55  in the state where no check in this repository can fail
   53  once the two vanilla ignores too are removed
```

Written up as `docs/PARITY-ROADMAP.md` §40. **Quote it from there or from a
commit message, not from recall.**

#### What was refuted

* **`corpus-surface.py`'s "21 client.rs can send" is 20.** It matches
  `sb::NAME` anywhere in a file that drives all five protocols, and
  `client_information` is registered in **both** play and configuration
  against the same class. Protocol-scoped now.
* **The duplicate-heading check could not see a duplicate heading.**
  `ci/check-doc-claims.py`'s `find_headings` returned a *set*. A stale
  `## 22.` line sat above the `## 26.` heading with the identical title, in
  `main`, and T0 reported `0 collisions, 0 drift` on every run. Fixed
  (`Counter` + `DUPLICATE-HEADING`), fixture added, stale line deleted.
* **My own first cut of the vanilla-handler resolution was wrong in the
  flattering direction.** A nested packet class inherits `handle` from its
  outer one, so `ServerboundMovePlayerPacket$Pos` resolved to a handler of
  size 0 and all four movement packets were reported as "vanilla ignores it
  too". `handleMovePlayer` is 799 instructions.

#### Nulls — do not re-run these

* **There is no serverbound recording anywhere and there cannot be one from
  the existing corpus.** `trace::observe` is called only from `client.rs`'s
  inbound frame loop. Do not go looking for an `emitted`-equivalent column
  for this half; it does not exist, and adding one is a recorder change.
* **`ci/corpus-floor.sh` has no premise here.** A census is not a comparison.
  The reproducibility reading printed in its place is byte-identical output at
  one commit — determinism, not resolving power.
* **The call-graph-to-sink analysis is *not* the collapsed instrument of
  §21.3.** It is bounded at depth 6 over the server crate with a deliberately
  tight sink set, and over-approximation here moves packets into `handled`,
  which is the flattering direction — so it is bounded by sink tightness
  rather than by resolution. It separates cleanly on this input (16/1/3).
* **`accepted` is small because `decoded` is small, not because we are
  careful.** One packet. Fifty-two are a tier worse.

#### The single next item, and what closes it

**Land the `PLAYER_LOADED` discriminator.** It is the cheapest of the three
named in §40.6, the only one that does not turn on a disconnect, and it
converts a packet that no check can fail on into one that can.

`handleAcceptPlayerLoad` is nine instructions and sets `markClientLoaded()`.
`hasClientLoaded()` then gates `handleMovePlayer` (offset 85, `ifne 92`,
`91: return`) and `handlePlayerInput` (offset 27). This server has **no arm
for the packet at all**, so in vanilla a client that has not sent it cannot
move or sneak, and here it can do both.

What it costs: a harness knob that defers `player_loaded` on the acting
connection, one scenario in a free range **below 790**, and its baseline row —
which needs a corpus run, and is the only reason it was not landed on this
branch. **Closes when that scenario records vanilla emitting no `move_entity`,
no waypoint and no step sound for a walk taken before the acknowledgement,
against this server emitting all three.**

Two more discriminators are written up in §40.6 with their bytecode:
`ACCEPT_TELEPORTATION` (a duplicated acknowledgement disconnects vanilla; the
only packet in the `accepted` bucket) and `KEEP_ALIVE` (an unsolicited one
disconnects vanilla).

#### Standing state a later agent should not re-derive

* **`CHUNK_BATCH_RECEIVED` looks checkable and is not.** Vanilla answers it
  through `PlayerChunkSender.onChunkBatchReceivedByClient(float)`, and
  `trace::is_housekeeping` drops `chunk_batch_start`/`chunk_batch_finished`
  before any decoder. Its whole clientbound effect is invisible by
  construction. Do not spend a turn writing a check for it.
* **`use_item` and `finish_dig` are dead verbs** — zero of the 93 checks use
  either. `use_item` is the sharper one: sendable, vanilla's `handleUseItem`
  is 89 instructions, and this server has no `USE_ITEM` arm anywhere.
* **`PlayerState::horizontal_collision` is decoded and never read.** The flags
  byte of every movement packet carries it. It does not show up as `accepted`
  only because the same byte's `on_ground` bit *is* read, so the packet is
  handled and one of its fields is not. A field-level census would be worse
  than this packet-level one.
* **T0 is red on `ci/check-use-probe-falsify.sh`, it is PRE-EXISTING ON MAIN,
  and it masks the last twelve gates in `ci/t0.sh`.** `real-table` fails with
  "the dumper swept 1 stances, `ci/checks/use-probe-dependent.toml` pins 12"
  plus three STALE fence-gate entries. **Controlled, not assumed**: a pristine
  `origin/main` worktree (`0679f236`) in its own remote dir
  (`~/rustmc-sboundctl`), tables generated the same way, fails at the same
  script with the identical message. This branch touches no table, no check
  and no dumper.

  The masking is the part worth acting on. `ci/t0.sh` runs that falsifier at
  line 229 and exits on it, so **twelve gate invocations after it have not run
  in any T0 since the red appeared** — among them
  `ci/corpus-surface.py --selftest`, in T0 since §21, and
  `ci/vanilla-reach-census.py --selftest`. A red that stops the run is a red
  that hides every gate behind it, which is this project's `--lib` defect
  wearing a different hat: the gate ran, the gate was red for one honest
  reason, and nobody could see that eleven other gates had stopped being
  evidence. `ci/serverbound-surface.py --selftest` is added at line 285 and is
  behind the same wall — **it is verified locally, not on the box.**
