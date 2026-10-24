# PRODUCT — what "perfect" means, decided 2026-07-22

Twelve product decisions made by the human (interview, 2026-07-22), informed
by intel/graveyard.md + intel/vri-lessons.md. These rank above convenience;
changing one is a human call. VISION.md/ORACLE.md stay the how; this is the
what.

## The one-sentence position

**The vanilla-parity server that proves it, the fastest one, and the
plugins to go with it — all three measured, none of them a footnote to
another.**
(Every graveyard project gets one sentence in the community's taxonomy;
this is ours. Nobody else can prove parity; everybody else claims fast.)

**Amended 2026-08-05.** The previous wording was "the vanilla-parity server
that proves it — and it's the fastest", which reads speed as a rider on
parity and does not mention the plugin layer at all. The owner set three
**co-equal** goals on that date: parity, speed, and the first-party
ecosystem. Speed was never secondary; the ecosystem is half the product
rather than an accessory to it. VISION.md's opening section carries the full
statement. Nothing below is repealed — decision 5 is still the parity bar,
decision 13 still governs the two modes, and decision 2's clean break from
Bukkit/Paper stands. What changes is that the third pillar is now named as
one: shipping both sides of the community is what makes decision 2
survivable, because for Minecraft servers the third-party ecosystem *is* the
reason people stay on the incumbent.

## Decisions

1. **Audience priority: (1) hosting companies, (2) small friend-run
   servers, (3) big public/technical servers, (4) minigame networks** —
   serve all four, build/market in that order. Hosting density is the
   spearhead; the toggleable-vanilla plugin layer serves #4 without
   diluting parity.
2. **Bukkit/Paper plugins: clean break, forever.** First-party plugins +
   importers (worlds, LuckPerms permissions) + ecosystem-around compat
   (datapacks, Velocity/Bungee proxies, community conventions). No Java
   plugin shim — it's the tar pit Pumpkin is currently walking into.
3. **Plugin system: tiered.** First-party = native Rust crates (trusted,
   in-tree). Third-party = **WASM sandbox**: any language, capability-scoped
   permissions, can't crash the server, hot-reload. Headline feature —
   no MC server has shipped it properly. Scripting tier (Lua) can layer on
   top of WASM later.
4. **Versions: latest only + 48h SLA.** The automated port pipeline is a
   survival organ (churn killed half the graveyard). Older clients =
   ViaProxy et al., their side.
5. **Parity: bug-for-bug vanilla, 99.9%+, receipts public.** QC, update
   order, duping included; per-bug config toggles exist but default to
   vanilla behavior. Exploit-closing is a config profile, not a fork of
   truth. Denominators published (region exactness + wire-check counts).
6. **Christmas deliverable: 1.0** — parity+speed receipts, 3 first-party
   plugins, plugin API v1 stable, docs site, one-command migration from
   vanilla/Paper. Public beta weeks earlier to soak on real servers.
7. **Success metric #1: real third-party servers running 24/7 on Dec 26.**
   Stars/press/revenue are instruments, not the goal.
8. **Hardware floor: 10-player survival runs WELL on 1 vCPU / 1GB VPS.**
   This is the hosting-density headline and a standing G5 bench config.
   **The memory half is CI-enforced from 2026-08-05: `ci/t2-memory.sh`, ten
   clients at the shipped view distance, inside a cgroup v2 `MemoryMax` of
   700 MB, so a breach is a kernel kill rather than a number a script chose
   to compare.** Until that date the parenthetical here read "CI-enforced
   like parity" and nothing in `ci/` set a memory limit, a `ulimit` or a
   cgroup — while the server was sitting at 2,957 MB with *one* player. A
   floor nobody checks reads as a floor somebody checks, so the claim is now
   written as what is actually enforced and what is not:
   * **enforced:** peak `VmRSS` for 10 co-located clients, view-distance 10,
     superflat, ≤ 700 MB. The gate self-tests by re-running under an
     impossible 32 MB and failing if that passes;
   * **NOT enforced, and neither is claimed:** players in *different* places
     (co-located players share tiles and are the cheapest arrangement);
     noise terrain rather than superflat; anything at all on **1 vCPU** —
     every figure is from a 24-thread box, and `docs/BENCH.md` lists the
     one-core measurement as not taken. "Runs WELL" is a statement about
     tick timing, which cannot be measured honestly until the tick loop does
     per-tick work; `docs/BENCH.md` declines to publish one and says why.
9. **Offline mode: exactly like vanilla** (online-mode=true default,
   configurable, no extra features, no judgment).
10. **Go public at first receipts** (~first real worldgen ratchet number,
    Aug/Sept) — "watch us climb to 99.9%" is the community ramp the
    Dec-26 server count needs. Supersedes "private until launch".
11. **Mods (Fabric/NeoForge): door left open** (human call, against
    orchestrator rec) — position publicly as "not planned; the WASM API is
    the extension story", never promise, never say never. Revisit post-1.0.
12. **Perfection is receipts, not adjectives** (meta-decision pervading all
    of the above): every claim — parity, speed, density, API stability,
    update SLA — ships with a public, reproducible measurement. The
    graveyard is full of adjectives.
13. **Two modes: parity mode and fast mode** (human call, 2026-07-25; names
    fixed 2026-08-02 and not to be varied). A server-wide toggle between
    bit-exact vanilla behaviour and a faster mode for operators who want
    speed more than exactness. Parity mode is the **default**; fast mode is
    opt-in. Wider than decision 5's per-bug toggles, which stay inside
    parity mode — this one is allowed to change results.

    **Feature coverage is identical in both modes**; only the
    exactness/speed trade-off differs. This is the parity server with a fast
    setting, not a reduced server with a parity option, and the naming says
    so — which is why no wording implying a cut-down build is used for it
    anywhere.

    **Sequencing: fast mode comes after all of parity mode, and none of it
    is built before then** (human call, 2026-08-02). No mode flags, no
    config switches, no second code path, no stubbed fast variant. There is
    exactly one implementation of each mechanic — the parity one. New
    systems are only required to keep their algorithm behind a boundary, so
    an alternative could sit beside it one day without restructuring.

    **What this does and does not license.** It makes a
    parity-breaking optimisation a *feature with a switch* instead of a dead
    end, which is a real unlock: several of the wins that would take us past
    vanilla are only unreachable because they change a value somewhere.

    What it must not become is an escape hatch for the position in the first
    line of this file. "Nobody else can prove parity; everybody else claims
    fast" — a fast mode that is characterised with adjectives hands the
    second half of that sentence back to the competition and starts eroding
    the first. So, binding, and a direct application of decision 12:

    - **Fast mode is measured by the same gates as parity mode.** Every
      golden gate runs in both configurations. Fast mode's number is not
      100.000000%, and the number it *is* gets published next to the
      parity one.
    - **Each divergence is named and bounded before it ships.** Not "may
      differ slightly" — "ore veins are placed by a cheaper sine, worst case
      one block, measured over N chunks". A divergence nobody has measured
      is a bug that has been renamed.
    - **Nothing lands in fast mode that has not first landed in parity
      mode.** The exact version is the reference the cheap one is measured
      against; building the cheap one first means there is nothing to
      compare it to.
    - **The speed claim in the one-sentence position refers to parity
      mode.** If we are only faster with exactness turned off, we are not
      faster, and saying so anyway is what half the graveyard did.

## "Perfect in every way" — the dimension checklist

- **Parity**: ≥99.9% region exactness; wire-check burn-down public;
  bug-for-bug incl. QC/update order; GAPS.md for every known miss (a
  documented gap is honest, a silent one is fraud).
- **Speed**: fastest vanilla-parity server on the G5 matrix (vanilla, Paper,
  Folia) under LOAD (15-bot scenarios, mob farms, redstone) — not startup/
  RAM vanity metrics; 1GB/1vCPU/10-player floor CI-enforced; regionized
  multi-core so big servers scale with cores.

  **Half true as of 2026-08-03** (`ci/bench-servers.sh`, docs/BENCH.md, two
  back-to-back runs on one box). On the one workload the four servers share
  — generating a 225-chunk square — rustmc now costs **less CPU than any of
  the three**: 2.30x less than vanilla, 1.39x less than Paper, 1.59x less
  than Folia per finished chunk. **Paper still wins wall clock**, 4.61 s
  against our 6.55 s, and wall is the binding target. The pipeline is not
  single-threaded and has not been since parallel decoration landed
  2026-07-29 — it peaks at 11.99 of 12 physical cores, but averages 2.83,
  and that serial fraction is where the wall gap lives. The load half of
  this bullet still cannot be measured at all: rustmc has no entities,
  block updates or random ticks, so there is nothing for a 15-bot scenario
  to load.
- **Usability**: single static binary, zero-config sensible defaults
  (auth on, vanilla world, working defaults everywhere); one-command import
  from vanilla/Paper (world + server.properties + LuckPerms); Pterodactyl
  egg at launch (hosting audience #1); clone→running-plugin <10 min;
  docs site at 1.0, not after.
- **Ecosystem**: WASM plugin sandbox + versioned API with semver-honest
  stability SLA (Minestom's #1 curse, inverted into a feature); template
  repo + cargo-generate starter; 3 first-party plugins as both product and
  API proof.
- **Operations (the admin's 2am)**: crash-free as a *stat* (soak-test
  hours published), graceful degradation under overload (shed view
  distance before TPS), actually-useful watchdog/timings built in,
  /tickprofile-class introspection from day one.
- **Trust**: live status page (SLA runs, parity number, bench history),
  AGPL, reproduce-everything instructions in-repo, no silent regressions
  (ratchets only move up).

## Standing anti-patterns (from the graveyard, enforce in review)

- No "playable preview" as a public peak — public moments are receipt
  moments on a ratchet.
- No vanity benchmarks, ever — if Pumpkin's 5ms startup is the comparison,
  we've already lost the plot.
- No compat shims for other platforms' APIs.
- No manual steps in the Mojang-update path.
- Complete verticals over broad previews (PicoLimbo > 60%-of-everything).
