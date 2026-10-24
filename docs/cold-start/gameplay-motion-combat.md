### COLD START — branch `gameplay-motion-combat`, off `main` (`b94f3ebf`), 2026-08-17

**The entity half of claim 070 is coded, unit-tested, T0 GREEN, and unrecorded.**
Worktree `~/grit/work-motioncombat`. Not merged — the orchestrator gates.

**WHAT THIS BRANCH CARRIES.** Two commits, both docs-then-code:

1. `3c373106` (was `7c4073ec` pre-rebase) — the **block half**:
   `crates/rustmc-server/src/fallable.rs`. `FallingBlock`'s unconditional
   delay-2 schedule, `isFree`, `tick`'s support test. `docs/PARITY-ROADMAP.md`
   §95 carries the full bytecode reading and the five-observation-per-client
   target trace for scenario `070`.
2. `3befe792` (was `2548be78` pre-rebase) — the **entity half**:
   `crates/rustmc-server/src/movement.rs` (new). `crate::physics::move_entity`'s
   first production caller — gravity, then the swept move, every tick, for
   `minecraft:falling_block` only. Landing decides `canBeReplaced` (via
   `crate::edits::Blocks::replaceable`) and `isFree(below)`, and sends the
   **two-update landing**: `Tracker::send_to_tracking_players` immediately,
   then `ChangedBlocks` queued — same relative order as vanilla's
   `ChunkMap.sendToTrackingPlayers` inside the entity tick vs. the next
   `ServerChunkCache.tick` broadcast. `status.rs` wires the fall spawn into
   the scheduled-tick loop (`tracker.add` is synchronous, so `add_entity`
   precedes the queued block change beside it) and calls
   `crate::movement::tick` after the tick's own `changed.flush`, with a
   second `flush` right after so a landing this tick reaches the wire this
   tick and not one late. `fallable::Cargo` is the new side table (entity id
   -> state string) `FallingBlockEntity.blockState`'s port needed: `Entity`
   itself carries only what the wire needs, and this state string is never on
   the wire at all.

**NAMED AND NOT PORTED**, each because nothing has measured it yet — full list
in `movement.rs`'s module docs, not repeated here: `ConcretePowderBlock`'s
water-touch conversion, `Fallable.onLand`, block-entity NBT copy, `cancelDrop`,
the moving-piston landing guard, the 100/600-tick despawn timeout, the
dropped-item arms on a landing that cannot place, and `canSurvive` read as
unconditionally true (`FallingBlock` and none of its 23 subclasses in 26.2
override it — checked via `javap`, not assumed).

**A pre-existing defect fixed in passing, not introduced by this branch:**
`fallable.rs` already had `mod tests` before its trailing `props()`/`tags()`
functions at `7c4073ec` — present since before this branch existed. That trips
`clippy::items_after_test_module` only under `--all-targets` (which activates
`#[cfg(test)]`; `--lib --bins` alone never sees `mod tests` at all and stayed
clean throughout). `ci/t0.sh` runs `--all-targets`, so this branch's first
full T0 was RED on it. Fixed by moving `mod tests` to the end of the file —
mechanical, no behaviour change, confirmed by an unchanged 604-pass test count
before and after.

**A T0 flake, not a defect — recorded so it is not re-chased.** The first T0
run (pre-fix) was independently RED on `readiness-falsify`'s
"still-starting capture already contains the announcement — the snapshot
raced the bind", while three other agents (`rustmc-piston`, `rustmc-gamerules`
and this run) were booting servers on the box at once. Re-ran
`ci/readiness-falsify.sh` alone, twice, both GREEN. Box contention, not a code
defect; every subsequent full T0 run (post-fix) has been clean on it too.

**VERIFIED, not assumed:** `cargo test -p rustmc-server --lib` — **604 passed,
0 failed, 1 ignored** (599 at the block-half commit + 5 new: 4 in
`movement.rs`, 1 `Cargo` round-trip in `fallable.rs`). `cargo clippy -p
rustmc-server --all-targets -- -D warnings` and `cargo fmt --check` both
clean. `ci/t0.sh` — **T0 GREEN**, rebased onto current `origin/main`
(`b94f3ebf`), no conflicts (this branch never touched `ci/t0.sh` itself, so
the conflict `docs/AGENT-BRIEF.md` warned about there did not occur). Box:
sayer-desktop (Threadripper 1920X, `threadripper` is an alias to this same
machine).

**Cross-checked against the reference trace already carried in §95** before
writing any landing code, not after: `070`'s five reference observations per
client are `block_update [1,0,0]->air` (the dig, already sent), `add_entity
falling_block [1.5,1.0,0.5] kind_id 51 data 118`, `block_update
[1,1,0]->air` (the sand leaving its own position), and `block_update
[1,0,0]->sand state_id 118` **twice**. This branch's `land()` writes to the
entity's own `blockPosition()` after that tick's move — which the reference
trace implies is `[1,0,0]`, one block below the sand's start, meaning the
scenario's world has solid ground at `y = -1` and the fall is short — and
sends the explicit update before the position is queued for the next
`changed.flush`, matching observations 4 and 5 in content and relative order.
**This is read off the trace already in §95, not a fresh recording** — the
actual verdict is unmeasured until the queued recording below runs.

**NULL, explicitly, so it is not re-chased:** the earlier claim on this
branch (corrected before I picked it up, §95) that `tracker.rs:652` defers
every pairing was wrong — that line is inside `Tracker::add`, not `tick`, and
`add` is the faithful port already called at spawn. No deferral defect exists
and none was reintroduced.

**WHAT REMAINS — the single next item.** Queue exactly one `ci/t2-gameplay.sh`
recording against this tree (corpus lock, role `run`, oldest-first queue) and
read scenario `070`'s verdict off it: `match` on both the declared and strict
denominator closes it; anything else — a wrong landing position, a missing
second update, a timing miss against `settle_ms = 2000` — is the next
diagnosis and belongs to whoever picks this up next, with the recording
itself as the evidence. **Do not re-run `gen-gameplay-tables.sh` or the setup
loop** — this worktree's `data/vanilla` and `oracle-work` are already
provisioned and stamped.
