# Task: In-repo wire harness v0 (oracle self-containment, PHASE0 P0-11)

> **DONE — built by the orchestrator pre-drop (2026-07-22), kept as spec
> reference.** Delivered: rustmc-proto varint/wire/frame/ids + oracle-runner
> status/join/check + ci/vanilla-server.sh + ci/t2-wire.sh GREEN vs real
> vanilla 26.2. Do not run this task.

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
8. Deliverables must be cherry-pickable: docs/log commits separate from code
   commits.

## Report format (final message — this is all the orchestrator reads)

- LANDED: commits + one-line what/why each
- EVIDENCE: gate output tails proving green (T0/T1 + task-specific)
- ESCALATED: blocked items, each with mechanism/evidence/blocked-on
- NOT TOUCHED: confirm main/other-trees/minecom untouched

## Read these sections (nothing else)

- docs/ORACLE.md — the top AMENDED block (self-contained decision) and §2
- docs/CRATES.md — "Workspace" tree + rustmc-proto responsibility line
- crates/rustmc-proto/src/lib.rs (whole file, it is short)
- tools/oracle-runner/ (current stub)

## The task

Build `tools/oracle-runner` v0: a Rust wire test client that connects to a
REAL Minecraft server on a host:port and performs, as separate subcommands:

1. `status <host:port>` — server list ping, print MOTD/version JSON.
2. `join <host:port> <name>` — full offline-mode login → configuration →
   play transition, stay connected 5 seconds, clean disconnect. Exit 0 only
   if the play state was reached.
3. `check <host:port> <check-file.toml>` — run scripted checks: send chat/
   command packets, assert on received packets (regex on chat, block-change
   at pos, etc.). Design the TOML schema yourself, minimally — it will grow.

Constraints:
- Protocol version = rustmc_proto::GAME_VERSION's protocol (26.2). Packet
  codecs you need that don't exist yet in rustmc-proto: implement them IN
  rustmc-proto (that is wave-1 work arriving early, good) — codecs are not
  a frozen interface, the `Packet` trait is (read-only).
- Verify against a real vanilla 26.2 server: `ci/vanilla-server.sh` (write
  it: downloads the vanilla jar to /oracle-work/, gitignored, starts it
  offline-mode on a port) — the jar is Mojang's, NEVER committed.
- Gate: a new `ci/t2-wire.sh` that boots the vanilla server, runs `status` +
  `join` + one trivial `check` against it, exits green. This script becomes
  G2's skeleton.
