# VISION — rustmc (working name)

> Working name only. The shipped name must NOT contain "Minecraft" (trademark).
> Candidates decided at launch; everything below uses "rustmc".

## The three pillars (owner-set 2026-08-05)

**Parity, speed, and the first-party plugin ecosystem are co-equal goals.
None of them is a supporting feature of another.** This supersedes any
earlier framing in this file that read parity as the goal and the other two
as things that serve it — an external survey read exactly that off our own
docs, which is why it is stated here first.

* **Parity** is measured against the **unmodified** vanilla jar, by
  instruments that are part of the project rather than by assertion.
  *Defer verification, never fidelity.*
* **Speed is not secondary and never was.** The first public release is
  expected to win on *published* numbers rather than on a favourable
  measurement setup; the second is the full optimisation pass. Standing
  caution, **corrected 2026-08-07**: boot time and resident memory were
  measured on 2026-08-05 and are no longer unmeasured, and the 2026-07-28
  bench that had this server slowest of four is superseded by the 2026-08-03
  one, where a finished chunk costs the least CPU of the four and the clock
  is still lost. Both windows, and the rule that they may not be compared to
  each other, are in `docs/BENCH.md`. What remains genuinely unmeasured is
  **tick timing under load** and **anything at all on one vCPU** — the
  binding half of the hardware-floor claim.
* **The first-party ecosystem** is half the product. Every zero-compatibility
  challenger that won did so because the incumbent's third-party ecosystem
  was not the reason people used the incumbent — and for Minecraft servers it
  is exactly the reason. The route out is to close that gap by **building**
  it, not by importing it. This does not reverse "Bukkit/Paper plugins: clean
  break, forever" (PRODUCT.md decision 2); it is what makes that decision
  survivable.

## What this is

The platform play: a from-scratch Minecraft-compatible server in Rust with a
**first-class plugin API** and **first-party rebuilds of the plugins admins
actually use** — shipped together, so a server owner's day-one checklist
(permissions, essentials commands, world editing) lights up without touching
third-party code. Open source end to end.

Three products in one repo:

1. **The server** — vanilla-parity gameplay, verified against a real vanilla
   oracle (see ORACLE.md), architected for regionized multi-core from day 0.
2. **The plugin API** — small, versioned, packet-visible. Designed by
   *extraction*: the API is whatever the first-party plugins genuinely needed,
   nothing speculative. Half of Bukkit's top-10 (Vault, PlaceholderAPI,
   ProtocolLib, Multiverse) are apologies for missing API surface — those
   become API features here, not plugins.
3. **First-party plugins** (launch set of three): essentials-core
   (homes/warps/kits/tpa/spawn), permissions (LuckPerms-grade groups/nodes),
   worldedit-core (selection + set/copy/paste/schematics — the 20% that covers
   ~90% of real usage).

## What this is NOT

- **Not Minecom.** Minecom is a separate, finished-to-its-stopping-point JVM
  project. It stays private/reference. Its role here is *oracle and executable
  spec* (ORACLE.md) — rustmc never links, ports, or copies Minecom code
  blindly; it passes Minecom's cross-language gates.
- **Not a Paper fork, not a Mojang derivative.** Zero Mojang code. Behavior is
  ported from observation + decompiled reference reading under the same
  discipline as Minecom (port behavior, cite the source class, never copy).
- **Not entry #17 on the Rust-server graveyard timeline.** Every prior Rust
  server died at the same spot: fun 10% done (protocol/NBT/chunks), then no
  verification rig and no stamina for the boring 80%. We start with the
  verification rig (that's the entire point of ORACLE.md) and the boring 80%
  is swarm work gated by it.

## License & money

- **AGPL-3.0 + CLA** from the first public commit (CLA lets the project dual-
  license later; payments/contracts run through Dad's company — operator is a
  minor). Free for everyone, forever, including unmodified commercial hosting.
- Revenue lanes (in activation order):
  1. **Partner-host referrals** — hosts pay for placement on the site/README/
     status page. Zero capital.
  2. **Paid support / update SLA** — the flagship contract: **patch releases
     green within 48h, contractually**; major versions get a public live
     tracker + target date, never a fixed-hour promise. Sold to hosts.
  3. Later, optional: marketplace rake, commercial licenses (via CLA).
- **The 48h SLA is a pipeline, not a promise:** a poller watches Mojang's
  `version_manifest_v2.json` (~10-min cadence), a release triggers the
  unattended pipeline (fetch jar → regen extracted data → run oracle suites →
  swarm ports red deltas → suite green → publish RC), and a **public live
  status page** shows the run in real time ("26.3 detected 14:32 · suite 84%
  green · ETA 6h"). The status page is simultaneously the SLA proof, the trust
  artifact, the release-day SEO magnet, and where partner-host placements
  live. Human = exception handler (phone ping on stall), never the trigger.

## Maintenance reality (design constraint, not aspiration)

- **Term time (September on): 5–10 h/week, concentrated in two post-IKEA
  weekend blocks.** Weekdays are school + study, untouchable.
- Operating rhythm: **Sunday night — queue autonomous work; Saturday
  post-shift — review, land, re-queue.** Agents work the week; the human works
  the weekends.
- **Minimum viable heartbeat** (worst-month floor): automated green bar + a
  monthly release note ≈ 2 h/month. The project must read as alive on the
  heartbeat alone.
- Compute: £40/WEEK hard max (user-set 2026-07-22, ~£5/day target); term-time near-zero baseline with
  £20–40 spikes on patch events (a few per year). 80–90% of wages go to
  savings — the plan never depends on spending them.

## Timeline (honest)

- **Phase 0 (pre-weights):** these docs, oracle wiring on the Minecom side,
  crate manifest + frozen interfaces, CI skeleton. Fable-driven, £0 marginal.
- **Phase 1 (K3 weights drop):** swarm sprint. 3-day core *if K3 delivers*;
  2–3 weeks to launch-grade regardless (hardening, plugins, docs).
- **Phase 2 (September→launch):** term-time weekend pulses burn down the
  long tail (parity to 99.9%, bench matrix wins, plugin API extraction).
- **Phase 3 — public launch, hard deadline Christmas 2026 (amended
  2026-07-22; supersedes "late August"):** launch when receipts are strong
  — receipts-first (self-contained repro: clone + one command reruns the
  parity proof and benches), partner-host outreach after. Headline claims,
  each measured not vibed: **99.9%+ vanilla parity** (region exactness +
  wire-check count, denominators public) and **fastest vanilla-parity
  server** (G5 matrix vs vanilla/Paper/Folia). Adoption work (beta
  servers, status page, first support contract) starts at launch, not
  after Christmas.
