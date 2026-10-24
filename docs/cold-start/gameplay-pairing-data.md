# Cold start — branch `gameplay-pairing-data`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gameplay-pairing-data` off `main` (`add7297a`), 2026-08-08

**Item: `PARITY-ROADMAP.md` §6 rank 4 — S1, `sendPairingData`'s middle three,
to convert `530-walking-away-drops-a-standing-player-from-view`.** Worktree
`~/grit/work-pairing`; box work in `~/rustmc-pairing`. Not merged — the
orchestrator gates.

**530 did not convert, and it cannot convert on server work.** One of the
three packets landed exact; one turned out never to be sent in this scenario;
the third compares a payload vanilla does not reproduce against itself.

```text
  before   declared 60/86 match  (20 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
           strict   56/86 match  (27 diverge, 3 absent, 0 vacuous, 0 inert, 0 error)
  after    declared 60/86 match  (20 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
           strict   56/86 match  (27 diverge, 3 absent, 0 vacuous, 0 inert, 0 error)
```

Before is `~/rustmc`'s 21:26/21:43 run, which reproduces the `add7297a` gate
figure independently; after is `~/rustmc-pairing` at `d69598d7`,
`T2-GAMEPLAY GREEN`. **The totals could not have moved and that is a
measurement, not an excuse:** searching both traces for `add_entity` with
`kind_id 156`, a *player* is paired in exactly one scenario in the whole
corpus. What did move is inside `530` — `explain --strict` goes from
vanilla 6 / ours 4 to vanilla 7 / ours 5, with `set_entity_data` now matching
at `99a4e94964a5df0a` and `add_entity` unmoved at `dc545a7f9dce13f5`.

**What landed.**

* **`set_entity_data` for a player pairing, payload derived from the jar and
  confirmed against a recording.** `09 03 41 A0 00 00 10 00 7F FF` — accessor
  9 (`LivingEntity.DATA_HEALTH_ID`, defined `1.0f`) as `FLOAT`, accessor 16
  (`Avatar.DATA_PLAYER_MODE_CUSTOMISATION`, defined `0`) as `BYTE` — digest
  `99a4e94964a5df0a`, which is what both vanilla recordings carry for both
  players. `crates/rustmc-server/src/synched.rs`, wired at
  `tracker.rs::pairing_bundle`.
* **The skin-parts byte, which this server parsed and threw away.**
  `configuration.rs` now keeps `ClientInformation.model_customisation` and it
  reaches the packet. Tested at the right offset — the two fields before it
  are a varint enum and a boolean.
* **`RUSTMC_TRACE_RAW`** in `tools/oracle-runner/src/trace.rs`: set it to a
  comma-separated list of kinds, or `all`, and every matching observation
  prints its tail as hex on stderr beside the digest recorded for it. Off
  unless set; stderr only; cannot affect a gate. It is what answered this
  item and it is what the next one needs.

**What I killed as a null — do not re-derive.**

1. **`set_equipment` is not one of `sendPairingData`'s middle three for 530.**
   §3 S1 says the actor holds something; the entity paired *on the actor's
   stream* is the observer, which holds nothing, and offsets 209-214 skip
   empty stacks. `explain --strict` shows no `set_equipment` in 530's
   projection at all. `observe_from = "actor"` and strict honours it.
2. **`update_attributes`'s payload cannot be matched, by us or by vanilla.**
   `ci/selfcheck-kind-stability.py` over the two full-corpus vanilla
   recordings of 2026-08-08: `update_attributes` 1 identical / 1 differing,
   the differing one being 530; `set_entity_data` 8 / 0; `add_entity` 2 / 6;
   `set_equipment` 5 / 1. A difference between two vanilla runs is
   conclusive. The mechanism is in the bytecode:
   `AttributeMap.getSyncableAttributes()` is `attributes.values().stream()...`
   over a lazily populated `Object2ObjectOpenHashMap` keyed by
   `Holder<Attribute>`, and `Holder$Reference` overrides neither `hashCode`
   nor `equals`.
3. **Digest-searching that payload does not work.** A meet-in-the-middle
   inversion over the 31 syncable player attributes at their supplier bases
   clears N ≤ 8 with no solution; the real payload carries item and
   creative-mode modifiers and a count that grows with session history.
   Measure it with `RUSTMC_TRACE_RAW` instead — a one-scenario subset gives
   three snapshots (`movement_speed`, and the two interaction ranges with
   their creative-mode ability modifiers) and a full corpus gives more.
4. **`set_equipment` is not reliably absent either.** Across four full-corpus
   vanilla recordings on 2026-08-08, 530's actor stream carried one in
   **one** run and none in three, because whether the standing observer is
   holding anything by scenario 70-odd depends on what earlier scenarios left
   in its inventory. So the *set of kinds* in 530's projection is
   run-dependent as well as one payload, and no amount of correctness in the
   subject converts a scenario whose reference cannot reproduce its own
   observation sequence.

**Standing state that is not mine but is now wrong:**
`~/rustmc/oracle-work/gameplay-selfcheck` is **empty**, as is every other
tree's on the box; the 26 recordings `docs/AGENT-BRIEF.md` describes did not
survive the move onto the Threadripper. `find ~ -type d -name 'selfcheck*'`
matches nothing else. The brief now says so.

**The single next item, and the result that closes it.** Decide what the
corpus should compare in `update_attributes`, and it is an instrument item
that belongs with R1-R3, not a server one. `ci/selfcheck-kind-stability.py`
runs on recordings that already exist, so every corpus run is another free
sample. It closes when either (a) the payload is canonicalised or dropped on
a stated rate, with a control proving the exemption is exactly as wide as it
claims — the shape `trace.rs`'s `add_entity` movement exemption already has —
or (b) it is written down that 530 cannot reach strict `match` and the
roadmap stops listing it as convertible. **Do not** reach the first by giving
this server an invented payload behind a widened projection: that converts
530 on packet identity alone, which is the failure §3 S1's own trap paragraph
names.
