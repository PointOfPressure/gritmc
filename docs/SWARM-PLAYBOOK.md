# SWARM-PLAYBOOK — running the K3 build sprint

How the build actually executes. Assumes ORACLE.md gates exist, CRATES.md
interfaces are frozen, CI.md queue is live. If any of those is false, the
sprint has not started — day 0 is not optional.

## Roles

- **Orchestrator (strongest available model — Fable-class):** merge-train
  arbiter, spec judge, interface-change authority, flake adjudicator. Does
  NOT write crate code during fan-out; every orchestrator hour spent coding
  is an hour the queue stalls. Runs on the already-paid subscription — the
  judge is free, only the horde is metered.
- **Workers (K3 via OpenRouter, post-weights-drop pricing):** one agent per
  crate/module, prompts built from the frozen docs. **AMENDED 2026-07-21:
  3 concurrent workers, not 20** — see WORKERS.md for mechanism and
  rationale (same tokens, fewer collisions, hot caches; wall-clock trade
  accepted). Wave structure below unchanged; waves just drain 3 at a time.
- **Human (weekend pulses):** direction, taste, launch decisions, spec
  disputes the orchestrator escalates. Phone-ping for pipeline stalls only.

## Wave structure

**Day 0 — no gameplay code.** Workspace + frozen interfaces (CRATES.md),
CI shards + merge queue live (CI.md), oracle adapters proven end-to-end:
one trivial "echo server" passing ONE Class-A PlayTest check through the
external-target adapter, one empty-world region write readable by
region-diff. The sprint's fate is decided here; a swarm pointed at an
unproven pipeline produces rework, not progress.

**Wave 1 — the spine:** proto (login→play flow), world (chunk store +
Anvil), data (registry load), server (tick skeleton + admission). Exit: a
vanilla client connects, sees a superflat, walks around; G2 smoke shard
green.

**Wave 2 — worldgen + entity core:** noise/terrain vs region-diff r3 ratchet
from zero upward; ECS + spawning; movement/physics vs the PlayTest movement
shards. Exit: r3 number within striking distance of Minecom's, entity smoke
shards green.

**Wave 3 — the boring 80% (behavior):** fan out per mechanic family
(combat, redstone, villagers, mob AI, containers, status effects…), each
gated by its PlayTest shard slice. This wave is where every predecessor
died; here it's a burn-down chart of shards going green. Largest wave,
most agents, least orchestrator drama if day 0 held.

**Wave 4 — plugin API + first-party plugins:** API extracted from what
essentials-core/permissions/worldedit-core actually need (CRATES.md
stance). Exit: three plugins run on the API alone, API doc-tests pass.

**Wave 5 — hardening:** full G3 r18, G4 determinism soak, G5 bench matrix
vs Paper/vanilla, admission/security pass (port Minecom's Connections
model), launch docs. Exit: tag candidate.

## Prompt patterns (proven in Minecom orchestration — use verbatim)

1. **Worktree-per-agent, port-per-agent.** First line of every prompt:
   create/enter your own worktree, export your own port. Two agents in one
   tree is the most expensive accident available.
2. **Read sections, not files.** Name the exact doc sections and line-ranges
   the agent needs. Token discipline is prompt discipline.
3. **Escalate, don't half-attempt.** If the task needs a stronger model or
   an interface change, the agent files an escalation entry (HANDOFF.md
   pattern) and stops. Half-correct commits cost more than escalations.
4. **Diagnosis-only tasks are legitimate** and often the highest-value ones
   (sulfur race, ore RNG, vibenilla determinism — all diagnosis-first, all
   landed the fix cheaply *after* the mechanism was proven). Do not let
   workers "fix" what isn't understood; the tick-fix→got-worse pattern is
   itself the tell.
5. **Invariant-style assertions, never instantaneous cross-thread reads;
   never widen tolerances.** (CI.md flake rules — put them IN the prompt,
   agents don't infer discipline.)
6. **Deliverables are cherry-pickable:** docs/log entries committed alone;
   code in separate commits; report format specified in the prompt (what
   landed, evidence, what's escalated).
7. **Tell the agent what NOT to touch** (main tree, other worktrees, the
   nether path…). Negative scope is cheaper than cleanup.
8. **Background-wait is an orchestrator job.** Workers implement and commit;
   the orchestrator drives final green-bars and tags (Minecom lesson:
   subagents stall on waiting-for-background work).

## Cadence

- **Summer sprint:** orchestrator sessions daily (evenings + Fridays);
  queue runs 24/7; burn-down chart of PlayTest shards green is THE metric —
  screen-record it from day 0 (it's also the launch video's third act).
- **Term time:** Sunday-night queue load, Saturday post-shift review/land/
  re-queue; pipeline + phone-ping between pulses (VISION.md heartbeat).

## Escalation ladder

worker (K3) → orchestrator (Fable-class) → human (weekend pulse or phone
ping). Each hop is one written artifact: the escalation entry states the
mechanism/evidence/blocked-on, never "it doesn't work". Spec disputes
(vanilla ambiguity, test-vs-product blame) are orchestrator calls; taste and
scope are human calls; nothing waits silently.

## Honest timeline & kill criteria

- **3-day core** (waves 0–2 + a slice of 3) *if* K3 is what launch-week
  claims say and hosting is stable. A weekend slower if not — plan the
  sprint window, not the miracle.
- **2–3 weeks to launch-grade** regardless of model quality: wave 3's long
  tail, plugins, hardening, docs, receipts post. The 3-day story is the
  video hook; the 3-week number is the plan of record.
- **Kill/pivot criteria (pre-committed, so momentum doesn't decide):** if
  after the first weekend the queue's green-rate says K3 output needs
  orchestrator rework on >30% of merges, drop to fewer/better agents (Opus
  workers on subscription) and stretch the timeline — the oracle makes slow
  progress durable, so the project survives any model disappointment. The
  only unrecoverable failure is gating on a rig that lies (flakes,
  nondeterminism) — which is why day 0 and the zero-flake policy outrank
  every feature.
