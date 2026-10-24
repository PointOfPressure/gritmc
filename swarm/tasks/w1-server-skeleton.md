# Task: Server skeleton — status + login→config→play vs oracle-runner (wave 1)

## Standing rules (stable prefix — do not edit per-task)

You are one of 3 worker agents building rustmc, a Minecraft-compatible
server in Rust. Non-negotiable rules:

1. You work ONLY in this worktree, on the current branch. Never touch
   `main`, other worktrees, `~/minecom`, or `vanilla-src/`.
2. Before every commit: `ci/t0.sh` and `ci/t1.sh` must pass. Never commit red.
3. Frozen interfaces (marked FROZEN in crate docs) are read-only. If your
   task cannot be done without changing one, STOP and write an escalation
   (see report format) — a half-done interface edit costs more than a stall.
4. Port behavior from the cited reference classes; never copy code. Cite the
   class in a comment at the porting site, Minecom-style.
5. No `static mut`, no ambient singletons, no HashMap iteration at
   behavior-visible sites (ordered containers there), no tolerance-widening
   in tests, no fast-math.
6. Assert invariants, never instantaneous cross-thread state.
7. If a test fails twice for the same reason you don't understand, stop and
   escalate with the evidence. Do not attempt a third blind fix.
8. Deliverables must be cherry-pickable: docs/log entries committed alone;
   code in separate commits.

## Report format (final message — this is all the orchestrator reads)

- LANDED: commits + one-line what/why each
- EVIDENCE: gate output tails proving green (T0/T1 + task-specific)
- ESCALATED: blocked items, each with mechanism/evidence/blocked-on
- NOT TOUCHED: confirm main/other-trees/minecom untouched

## Read these sections (nothing else)

- crates/rustmc-proto/src/ (all files — the codecs you'll reuse; ids.rs
  header explains regeneration)
- tools/oracle-runner/src/main.rs (the client that will grade you — its
  login_to_play flow is the exact packet sequence your server must drive
  from the OTHER side)
- crates/rustmc-server/src/lib.rs (Scheduler/RegionMessage are FROZEN;
  the cli module documents the CLI contracts)
- docs/CRATES.md regionization rules 1–2

## The task

`rustmc run --port <p>`: accept TCP connections and drive the server side
of exactly what oracle-runner's client side already proved against vanilla:

1. status: status_request → status_response JSON (version name/protocol
   taken from GAME_VERSION + the protocol number oracle-runner reports for
   vanilla — pin it as PROTOCOL_VERSION in rustmc-proto with a comment),
   ping → pong.
2. login (offline only): hello → login_compression (threshold 256) →
   login_finished → expect login_acknowledged.
3. configuration: send select_known_packs (empty), finish_configuration;
   handle the acks; keep_alive every 10s both here and in play.
4. play: send a minimal login (play) packet + player_position; keep the
   session alive (answer serverbound keep_alive by expecting them —
   client echoes ours). An empty-superflat chunk payload is NOT required
   for this task — oracle-runner join only asserts reaching play and
   surviving; chunk sending is the next task.

Gate (this closes SWARM-PLAYBOOK wave-0's rustmc half): a new
`ci/t2-selfwire.sh` that builds rustmc, boots it on a port, and runs
`oracle-runner status` + `oracle-runner join` against it, both green.
Threading: single accept loop, one thread per connection is FINE for v1
(admission/regionization arrives later; do not build an executor here).
