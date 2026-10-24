# PHASE 0 — status tracker (drop day: 2026-07-27)

Day-0 rule (SWARM-PLAYBOOK): a swarm pointed at an unproven pipeline
produces rework, not progress. Everything here is prerequisite to wave 1.

## Done (2026-07-21)

- [x] **P0-1 Workspace scaffold** — 8 crates + 3 plugins + 2 tools, builds
      green, clippy clean, doc-tests pass; frozen interfaces drafted
      (`ChunkAccess`, `Canvas`, `ChunkGenerator`, `EntityView`,
      `BlockBehavior`, `Event`/`Plugin`/`Server`, `Scheduler`,
      `RegionMessage`, `Packet`); pinned toolchain 1.96.1; workspace lints
      (deny `static_mut_refs`, warn hash-iteration).
- [x] **P0-2 Worker harness** — kimi CLI headless confirmed (print mode +
      OpenAI-compatible providers); `swarm/worker.sh`, task TEMPLATE,
      WORKERS.md. 3-worker decision recorded.
- [x] CI T0/T1 runnable now; T2 gated on manifest; G4 script ready for
      first genregions output.

## Open — ordered, with owners

- [x] **P0-3 Minecom PlayTest-external** — DONE 2026-07-21 with a major
      finding (E1): PlayTest had NO wire bots (in-process TestPlayer/
      FakeConnection) — ORACLE.md §2 premise was false; §2 rewritten.
      Delivered on branch `playtest-external` (~/minecom-w-external,
      unmerged): real WireClient (Minestom codecs), 10 external checks
      green ×2 against cold-booted Minecom on :25700, manifest 1081 checks
      → A=6/B=611/C=464 with per-check conversion pricing, 3 B→A proven.
      G2 consequence: wire coverage is a wave-1+ burn-down (from 10 up),
      not a flip-on; C-class (464) covered via golden vectors instead.
- [x] **P0-4 region-diff `--command` patch** — DONE 2026-07-21, branch
      `regiondiff-command` (~/minecom-w-regiondiff, unmerged). Proof:
      --jar and --command runs bit-exact identical r3 99.049603% (seed
      20260708). Real argv contract is positional
      `--genregions <seed> <radius> <cx> <cz> <dim>` — ORACLE.md §3
      amended to match. Merge to minecom main on next review pulse.
- [x] **P0-5 goldengen dumps** — DONE 2026-07-21: 19 golden JSONs (2.9MB)
      under crates/{worldgen,data,behavior}/golden/ (RNG streams 5 seeds ×
      10k × 2 families + derived/fork paths, map colors, sulfur archetypes,
      difficulty/enchant-cost/beacon/conduit vectors). Minecom-side dump
      entrypoint on branch `goldengen-dumps` (~/minecom-w-goldengen,
      unmerged; regen = one command, in golden READMEs). Verified: dump
      double-run byte-identical, agent's independent Python cross-check,
      orchestrator's own java.util.Random replay (3 seeds × 100 EXACT).
      Skipped-with-record: explosion math (not pure, PlayTest covers),
      enchant offer-selection (needs full data load).
- [x] **P0-6 Interface freeze review** — DONE 2026-07-22, **INTERFACES
      FROZEN** (declaration in CRATES.md). Gaps fixed pre-freeze:
      ChunkAccess +biome/set_biome/light, EntityView +kind, data crate
      +BiomeId/EntityKindId/LightKind. New policies: additive defaulted
      methods ≠ interface change; WASM-boundary rule binding on rustmc-api.
      T0+T1 green post-amendment.
- [x] **P0-7 Threadripper CI** — DONE 2026-07-22, fully closed. Ubuntu
      Server 24.04 dual-boot (Windows kept; GRUB
      defaults Ubuntu for unattended wakes). 24 threads / 62GB / 147GB
      NVMe. `ssh threadripper` (192.168.1.122, key auth; hub DHCP
      reservation SET 2026-07-22 — EE Hub Manager, "Always use this IP:
      YES", assignment Static), wake via `~/bin/tr-wake` (WoL verified:
      poweroff→SSH in 44s), idle-poweroff timer (20min, respects
      /run/ci-busy + build procs), rustc 1.97.1 + JDK 21 + maven,
      repo rsynced, **T0+T1 GREEN**. Curfew = MIDNIGHT: root cron 23:50
      `shutdown -h now` installed+verified; orchestrator rule: no TR jobs
      that can't finish by ~23:30. Runner mechanism (systemd timer vs
      self-hosted GHA) decided post-P0-9.
- [ ] **P0-8 API key** — ~/.config/openrouter/ is EMPTY (keyfile from
      memory is gone). Human: re-create key on drop day, set £5–8/day (£40/WEEK hard max)
      per-key limit at creation. Moonshot platform key as caching fallback.
- [x] **P0-9 GitHub private repo** — DONE 2026-07-22:
      github.com/PointOfPressure/rustmc (private; **renamed since — the live
      remote is `PointOfPressure/gritmc`, with the plugin sibling at
      `PointOfPressure/grit-plugins`. Corrected 2026-08-07; the old URL does
      not resolve**), pushed via HTTPS+gh
      (laptop ed25519 key is NOT on GitHub — SSH remote fails, same as
      minecom). Threadripper pulls via read-only deploy key
      (threadripper-readonly, id 158056156), tracks origin/main. Runner
      decision: orchestrator-driven over SSH (wake→pull→gates), no GHA
      daemon unless the queue outgrows it. Go-public trigger: first
      receipts (PRODUCT.md #10). Wave-0 task files pre-written:
      swarm/tasks/w0-{wire-harness,region-differ,golden-dumper}.md.
- [x] **P0-10 Extracted data** — DONE 2026-07-21: 1,501 files (12M) copied
      to `data/vanilla/` from minecom @ a1d9d40, gitignored (Mojang-derived,
      repo goes public); provenance + regen command in data/README.md.
- [~] **P0-11 In-repo oracle v0 (from-scratch amendment, 2026-07-22)** —
      2 of 3 BUILT BY ORCHESTRATOR pre-drop (same day):
      **(a) wire harness DONE** — rustmc-proto varint/wire/frame/ids (ids
      from Mojang's own data-gen report) + oracle-runner status/join/check
      + ci/vanilla-server.sh; `ci/t2-wire.sh` GREEN vs real vanilla 26.2
      (login→config→play, keepalives, /me check round-trip).
      **(b) region differ DONE** — rustmc-world nbt+anvil readers,
      tools/region-differ scan/diff, ci/gen-vanilla-region.sh (≤r7);
      `ci/t2-regiondiff.sh` = vanilla-vs-vanilla 100% gate. 26.x layout
      finding: overworld saves under world/dimensions/minecraft/overworld/.
      **(c) golden dumper OPEN — deliberately left as the harness-A/B task**
      (swarm/tasks/w0-golden-dumper.md) when the OpenRouter key lands.
      Wave-1 task files pre-written: w1-server-skeleton (closes wave-0's
      rustmc half), w1-data-registry, w1-anvil-writer.

## Drop-day runbook (2026-07-27) — target: firing within the hour

1. Confirm K3 weights live; survey hosts (price + caching support);
   `openrouter-test` sanity call.
2. Create swarm key, £5–8/day per-key limit (budget: £40/WEEK hard max, prepaid ~£10 top-up), write keyfile; point kimi CLI provider
   config at chosen host; one `kimi --print -p "hello"` smoke test.
3. `ci/t0.sh && ci/t1.sh` green on laptop + Threadripper.
4. Wave-0 proof (SWARM-PLAYBOOK): echo-server passes ONE Class-A check via
   the in-repo wire harness; empty-world region write readable by the
   in-repo differ (P0-11 tools — building them IS the first swarm work if
   not done pre-drop).
5. First 3 tasks out (wave 1: proto login→play, world chunk store + Anvil,
   data registry load). Screen-record the burn-down from the first task.
