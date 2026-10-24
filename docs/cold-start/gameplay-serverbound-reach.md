# Cold start — branch `gameplay-serverbound-reach`

This branch's own append-only history. A new entry goes on the end; nothing
above is edited.

---

### COLD START — branch `gameplay-serverbound-reach`, off `main` (`a106314d`), 2026-08-13

Worktree `~/grit/work-sbreach`, remote dir `~/rustmc-sbreach`. Continues
`corpus-serverbound-surface`, which is **still unmerged**.

#### The dependency that will bite the next reader first

`ci/serverbound-surface.py` is **not in this tree**. It lives on the unmerged
branch `corpus-serverbound-surface` and was copied into `ci/` here, untracked,
purely as an instrument — never `git add`ed, so that when that branch merges
there is no add/add conflict and no second copy to drift. Every figure in
`docs/PARITY-ROADMAP.md` §49 came out of it.

**So §49's numbers cannot be reproduced from this tree alone.** Copy the script
back in the same way, or wait for the merge:

```bash
git show corpus-serverbound-surface:ci/serverbound-surface.py > ci/serverbound-surface.py
chmod +x ci/serverbound-surface.py && ./ci/serverbound-surface.py
```

One edit is needed after that and is the point of §49.5: drop
`ACCEPT_TELEPORTATION` from the script's `ACCEPTED_REASON` table, or it refuses
— correctly — because the packet has gained a reader.

#### What was measured

```text
                            before   after
   in the protocol             69      69
     HANDLED                   16      17
     ACCEPTED                   1       0
     DISCARDED                  3       3
   no check can fail on        55      54
```

**16 of 69 could make a check go red before, 17 after.** `handled` is a
*static reachability* claim and is not coverage — see §49.6 — so the number
that matters is the mutation evidence, not this one.

#### What `ACCEPT_TELEPORTATION` turned out to be

A live defect with a second one behind it, not missing coverage. It was decoded
into `PlayerState::acknowledged_teleport` and **read by nothing outside
`player.rs`'s own unit tests** — the arm could have been deleted with every
check staying green. Behind it, the field's doc comment claimed an
unacknowledged client's "positions are not yet authoritative", describing
behaviour no line of this server implemented.

Ported from `handleAcceptTeleportPacket` and `updateAwaitingTeleport`, offsets
in §49.2. `awaitingTeleport` is never cleared and `awaitingPositionFromClient`
is, which is why acknowledging the same id twice disconnects.

#### The harness capability this branch adds

`trace::Who::Probe` — a third connection a scenario opens with `probe_join`,
measured on its own socket, dropped by `run_one` when the scenario ends. It
exists because the actor and observer replay *every* scenario, so any check
whose content is "vanilla hangs up on this client" would destroy the rest of
the corpus. That is a class, not a one-off: `KEEP_ALIVE` is the next member
(`handleKeepAlive` offset 51 — the disconnect is skipped only for
`isSingleplayerOwner`, and the gate boots a **dedicated** server, so it fires).

#### Nulls — do not re-run these

* **A mismatched-id acknowledgement discriminates nothing on its own.** Vanilla
  ignores it at offset 20 and so do we, because nothing is outstanding once the
  join teleport has been answered.
* **There is no mid-session teleport to hang a non-disconnecting check on.**
  `/tp` is not implemented (`commands.rs` has `/list` and `/seed`), and the
  harness's own `goto` walks at a legal speed on purpose, so nothing provokes a
  rubber-band. This is why the discriminator is a disconnect and why the probe
  had to exist.
* **The probe cannot be observed from the actor.** Workspaces are at
  `-(index+1)*32`, so a probe joining at spawn is thousands of blocks outside
  the actor's view distance and is never entity-tracked. Its own socket is the
  only place its disconnect is visible.
* **`ci/t2-gameplay.sh` has no `max-players` override** — vanilla's default 20
  applies, so a third connection is not a problem there.

#### Tooling added on the way

`RUSTMC_CHECKS=<dir>` runs `ci/t2-gameplay.sh` over a smaller directory,
because the corpus lock serialises full runs at ~35 minutes and was three deep
while this branch was being written. **Renamed from this branch's own
`T2_GAMEPLAY_CHECKS` on rebase, 2026-08-17**: main built the identical feature
under `RUSTMC_CHECKS` and it is the name the rest of `ci/t2-gameplay.sh` (the
`SUBSET OK` banner further down) already reads. It prints a banner on every
run and **nothing may be baselined or reported as T2-GAMEPLAY GREEN from a
subset** — it cannot see a check it did not run.

#### The partition of the 53, and a refutation

**20 drivable today / 25 capability-blocked / 8 dead or invisible** — §49.7 has
the per-packet evidence. Two things a later agent should take from it:

* **`USE_ITEM` 0x43 is the cheapest red in the repository and costs one TOML.**
  `Step::UseItem` already exists and already puts the packet on the wire; this
  server has no arm for it anywhere. Nothing has to be built to make that check
  *fail*.
* **The recorder does not need a decoder.** `trace::observe` falls back to
  `packet:0x{id:02X}` plus a body digest, so "can the recorder see it" is only
  the `is_housekeeping` question. That is what makes group A twenty rather than
  about eight.

**REFUTED — `PLAYER_LOADED` is not the next item.** The previous branch's cold
start names it as "the cheapest of the three" and "probably the best first
item". It is group C: `handleAcceptPlayerLoad` emits nothing ever, the flag it
sets can never be cleared once the join has sent the packet, and observing its
gate means withholding the packet and racing `clientLoadedTimeoutTimer` on the
shared connection — an intermittent check, which is worse than none. §49.8.

#### The single next item, and what closes it

**`KEEP_ALIVE`, on the probe.** An unsolicited keep-alive disconnects vanilla
and is ignored here. It needs one verb (`probe_keep_alive_unsolicited`), one
scenario and its baseline row, and it reuses the whole probe mechanism this
branch built. Closes when that scenario records vanilla emitting `disconnect`
and this server emitting nothing, and then goes green once the arm is ported —
verified by mutation, not by the census's `handled` count.
