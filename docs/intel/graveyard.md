# GRAVEYARD — the Rust Minecraft server field, mined 2026-07-22

Sources: dayssincelastrustmcserver.com timeline (by GoldenStack; list is loose
— includes non-servers), GitHub API status pulls (2026-07-22), project sites,
Minestom Discord export mining (see vri-lessons.md). "Last push" is the
objective aliveness signal.

## The list

| Project | Started | Last push | Stars | Furthest point / niche | Verdict |
|---|---|---|---|---|---|
| Feather | 2019-09 | 2024-04 (real activity died ~2022) | 2,707 | protocol+chunks+ECS+some gameplay; no worldgen parity | **DEAD** — the archetype: "fun 10%" done, boring 80% never |
| MCHPRS | 2020-06 | 2026-07 | 2,252 | redstone-ONLY server, compiles circuits to graphs, orders of magnitude faster than vanilla | **ALIVE — niche success** |
| Bamboo | 2022-01 | 2024-01 | 35 | minigame-oriented server lib | DEAD |
| Valence | 2022-09 | 2026-06 | 3,254 | library/framework (Minestom-of-Rust), Bevy ECS | **Barely alive** — library approach survives, momentum lost after ECS rewrite churn |
| Hyperion | 2024-03 | 2025-10 | 1,054 | 10k-player custom-event engine (not vanilla) | Stalled |
| Pumpkin | 2024-07 | 2026-07 (daily) | **8,220** | vanilla-ish server; own API + PatchBukkit compat attempt; Bedrock cross-play ambitions | **ALIVE — the incumbent** |
| FerrumC | 2024-09 | 2026-07 | 2,338 | vanilla reimpl, big hype spike 2024 | Alive, slower |
| PKMC | 2025-01 | 2025-06 | 43 | "yet another" | DEAD |
| Oxide | 2025-03 | 2026-07 | 72 | solo pure-Rust reimpl | Alive, solo/small |
| KittyMC | 2025-03 | 2025-05 | 16 | toy | DEAD |
| PicoLimbo | 2025-05 | 2026-06 | 495 | limbo/waiting server ONLY | **ALIVE — niche success** |
| Steel MC | 2025-10 | n/a | — | docs site only | Embryonic |
| Spinel | 2025-11 | 2026-07 | 9 | framework | Embryonic |
| Temper | 2026-04 | n/a (site) | — | "stupidly fast" ECS marketing; explicitly dismisses decompile-porting → parity is a non-goal | Early, perf-first |
| Infrarust | 2026-06 | active | — | **not a server** — reverse proxy (site list is loose) | n/a |
| Coral | 2026-06 | 2026-07 | 20 | 1.8.x-only (protocol 47) | Embryonic, niche |

## Failure patterns, ranked by body count

1. **Death after the "initial playable state."** Feather, Bamboo, PKMC,
   KittyMC, FerrumC's slowdown — and named verbatim in the Minestom Discord:
   *"there have been a lot of server implementations, and all gave up after
   the initial playable state."* Protocol+chunks+walking is the fun 10%;
   nothing forces the boring 80%. **Countermeasure (already ours): the
   oracle IS the forcing function — a burn-down chart of failing checks
   makes the boring 80% legible, delegable to a swarm, and visibly finite.**
2. **No verification story → parity claims unfalsifiable → parity debt.**
   Zero projects on this list can *prove* a parity number. Glowstone (JVM
   precedent) died of exactly this: *"they struggle because of vanilla
   parity."* **Countermeasure: the 99.9% receipt with public denominators is
   the category-defining move nobody else can copy quickly — none of them
   have a Minecom.**
3. **Perf-marketing on vanity metrics instead of gameplay.** Pumpkin
   headlines 5ms startup and 100MB RAM; Temper headlines architecture
   adjectives. Startup time and idle RAM are trivial for ANY rewrite; TPS
   under 15 loaded players with real mob AI/redstone is the honest metric
   and nobody publishes it. **Countermeasure: G5 bench matrix vs
   vanilla/Paper under load, published with harness source.**
4. **Version churn treadmill.** Mojang ships ~2 major + N patch releases a
   year; each one resets protocol+data+behavior. Solo projects skip a
   version, fall behind, die (several list entries pinned to old protocols;
   Coral chose 1.8 to opt out entirely). **Countermeasure: the automated
   port pipeline + 48h-SLA is a survival mechanism first, product second.**
5. **Bukkit-compat tar pit.** Pumpkin is attempting PatchBukkit; historically
   (JVM: Glowstone) chasing another platform's API surface consumes the
   project. **Countermeasure: clean break (decided), first-party plugins
   cover day-one, importers not shims.**
6. **Solo bus-factor + burnout.** Nearly every corpse was one maintainer's
   motivation. **Countermeasure: swarm + oracle makes progress durable and
   restartable; the project survives any individual (including pauses).**
7. **Library-vs-product ambiguity.** Valence survives as a library but
   nobody RUNS a Valence server; libraries don't die but also don't ship
   servers. rustmc is a *product* (a binary admins run) with a library
   inside (the plugin API) — keep that order.

## Survivors and why

- **MCHPRS**: picked ONE vertical (redstone), went 100x deeper than vanilla
  on it, owns that community forever. Lesson: verticals with receipts beat
  horizontals with promises.
- **PicoLimbo**: smallest possible complete product (limbo server), shipped,
  useful, alive. Lesson: complete beats ambitious.
- **Pumpkin**: community mass (8.2k stars) via accessibility framing
  ("single executable, no Java") + relentless activity. Lesson: the
  accessibility pitch works; stars follow momentum. But its parity is
  unverified and its compat bet is a tar pit — that's the flank.
- **Minestom itself (JVM)**: survived 6+ years by *refusing* vanilla parity
  (library, latest-version-always). Its community's verdict on staying
  current: *"minestom is latest release and most other server impls
  aren't."*

## Implications for rustmc

1. Differentiation is **proof, not speed claims**: verified 99.9% + loaded
   TPS matrix + public status page. Every competitor markets adjectives.
2. Pumpkin owns "fast & easy"; rustmc must own **"correct & fast, proven"**
   — and reach hosting companies with the density number (10 players / 1GB /
   1 vCPU) that Pumpkin's RAM headline gestures at but doesn't gameplay-prove.
3. The 48h-SLA pipeline is the anti-churn survival organ — build it as
   infrastructure, not marketing.
4. Ship complete verticals in order (PicoLimbo lesson): a perfect
   superflat-survival server beats a 60%-everything server at every stage.
5. Never ship a "playable preview" as the peak — the graveyard is full of
   playable previews. Public milestones are receipt-moments on the ratchet,
   not playability moments.
