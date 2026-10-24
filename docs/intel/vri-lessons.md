# VRI LESSONS — mined from the Minestom Discord exports, 2026-07-22

Source: DiscordChatExporter dumps in ~/Documents/DiscordChatExports
(vri-general 14,898 msgs, showcase 8.2k lines, general ~500k msgs, help 96k
lines; off-topic skipped as non-project). Parser:
extract_discord.py → flat text → keyword passes. Quotes are verbatim
(authors/timestamps not preserved by the parser for this export format;
message IDs retrievable from the HTML if ever needed).

## Why vanilla-reimplementation efforts died (their own words)

- *"Well there have been a lot of server implementation, and all gave up
  after the initial playable state"* — THE pattern, stated as common
  knowledge in the community that hosts these attempts.
- *"thats glowstone then, and they struggle because of vanilla parity"* —
  parity debt killed the JVM predecessor; everyone knows it.
- *"But I think a bunch of people that tried contributing lost motivation"*
  — bus factor + no visible progress metric.
- *"I got to repeaters then gave up"* — redstone kills individuals at the
  component level.
- *"Considering Minestom isn't feature frozen and breaks stuff frequently
  I'm not sure if this should be abandoned until Minestom's more stable to
  avoid a treadmill of chores"* — building on a moving foundation compounds
  churn. (rustmc owns its whole stack — no upstream treadmill except Mojang.)
- *"it kinda leaves a sour taste to abandon Java for a lower level language
  and yet have to gate your server behind a Java proxy anyway"* — standalone
  binary matters; don't require a JVM proxy for anything core.

## Effort map (their consensus, cross-checked against our experience)

- **#1 Worldgen** — *"BUT worldgen is the #1 Hardest"*, *"I would argue that
  worldgen is probably the hardest thing worth actually doing"*, *"world gen
  is definitely hard but i'd personally rather have a superflat with
  otherwise good vanilla parity"*. → **rustmc starts with this SOLVED**
  (Minecom 99.38% + bit-exactness discipline + the ratchet harness). This is
  the single biggest inherited advantage; nobody in the field has it.
- **#2 Redstone** — *"Redstone is the hard part, It is pretty difficult to
  implement parity"*, *"redstone is the hardest thing but I also think it's
  impossible"*, *"Implementing a actually good system for block updates is a
  nightmare"*. The kill mechanism: update ORDER semantics, not components.
  → Port Minecom's tested update-order behavior; bug-for-bug (QC included)
  per product decision; MCHPRS proves Rust redstone can be world-class.
- **#3 Mob AI / pathfinding** — dissonances from vanilla behavior get
  noticed and called out per-mob in threads.
- **Villages/villagers** — *"Villages are just stupidly done by mojang and
  it makes implementing them quite hard."*
- **Sneaky long tail**: shaped-recipe permutations, lighting engine,
  inventory edge cases (*"i should really finish porting my inventory
  changes to master so we can finally get full parity"*).
- **Easy/fun (the trap)**: protocol, packets, chunk sending — *"Feel like a
  puzzle"* — this is the 10% every corpse finished.

## What the ecosystem actually wants (signal from wants/complaints)

- **Latest version, always**: *"minestom is latest release and most other
  server impls aren't"* — staying current is a named competitive advantage.
  (Supports the 48h-SLA bet directly.)
- **Real perf under load**, not startup benchmarks — perf talk in vri is
  about TPS with players/entities, view distances, thread scaling.
- **No Java toolchain anywhere** in the operator path (single binary).
- **Open source expected by default**: showcase pattern — *"yall should
  release the src for these stuff"*, *"is it open source?"* asked of
  everything within minutes.
- **Sensible defaults ship adoption**: the Sabre showcase (server jar with
  "sensible defaults": auth ON, superflat, proxy support, Optifine toggle)
  was the celebrated formula — defaults + toggles, no config homework.
  Matches decisions: online-mode default ON, per-bug toggles default
  vanilla.

## Version-churn survival

- Minestom's own API instability was a top complaint: *"Thanks for another
  breaking change without warning"*, *"api breaking changes are still not
  uncommon"*, and would-be contributors feared maintenance burden through
  breaking changes. → **rustmc plugin API: versioned, semver-honest,
  breaking changes batched into announced majors with migration notes; the
  API's stability is a FEATURE with the same receipt discipline as parity.**
- Mojang-side churn is the other treadmill (*"Breaking changes made by
  mojang"*) — the automated port pipeline is the answer; projects that
  handled updates manually skipped one and died.

## API lessons from Minestom (praise to steal, curses to avoid)

- Praise: small core, everything-is-extension, latest-version-always, no
  vanilla fluff you didn't ask for. rustmc's toggleable-vanilla via the
  plugin/config layer serves the same crowd (audience #4).
- Curses: breaking changes without warning; jitpack/build-tool friction in
  onboarding (help channel's top recurring themes are build/setup and
  entity/event boilerplate); docs lag. → Day-one docs site + template
  plugin repo + `cargo generate` starter; onboarding time is a usability
  metric (target: clone → running plugin in <10 min).
- The taxonomy the community itself uses (steal for positioning): *"Feather's
  goal is a full vanilla reimpl… MCHPRS's goal is to reimplement redstone but
  faster… Valence's goal is to be like Minestom"* — every project gets ONE
  sentence. rustmc's must be: **"the vanilla-parity server that proves it —
  and it's the fastest."**

## Implications for rustmc (delta to current plan)

1. Worldgen-first receipts: our strongest card is the field's acknowledged
   hardest problem, already solved. Lead every public artifact with it.
2. Redstone = second receipt and the technical-audience credibility gate;
   budget it as a Tier-1 wave-3 vertical with its own burn-down, and test
   update-order semantics explicitly (the community's named killer).
3. Plugin API stability gets SLA-grade discipline (versioned, migration
   notes, never silent breaks) — it's the #1 curse named against Minestom.
4. Onboarding is a benchmark: single binary, sensible defaults, plugin
   template, <10-min first-plugin. The Sabre formula, done natively.
5. No JVM anywhere in the operator experience (oracle tooling keeps its JVM
   only on OUR CI side).
