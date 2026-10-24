# gritmc

A from-scratch Minecraft-compatible server in Rust, with **three co-equal
goals** (owner-set 2026-08-05). None of them is a supporting feature of
another, and the ordering below is not a ranking:

1. **Parity.** Bug-for-bug vanilla behaviour, measured against the
   **unmodified** vanilla jar by instruments that are part of this repo —
   the gameplay-parity oracle and the worldgen differ — rather than
   asserted. Denominators are published and every figure carries its
   caveats. **Worldgen at r=16 no longer carries an exactness percentage, and
   the reason is the interesting part.** Over a 33x33-chunk overworld window
   at seed 20260722 — 1,089 chunks, 107,053,056 blocks, full height, no mask —
   this server writes **2,450 blocks that match none of eight independently
   generated runs of the unmodified jar**, with the chunk order held fixed on
   both sides. The same instrument charges a run that *is* unmodified vanilla
   between 2,202 and 5,263 blocks over the same references. **Our number sits
   inside that range**, so what the window now says is not a level but a
   comparison it can no longer make: over 107 million blocks nothing here can
   separate this server from a vanilla run of the same seed. That is a real
   and checkable claim and it is deliberately not a percentage — an exactness
   quoted against a floor it sits under is corroboration, not a measurement.
   (It is also not a fidelity win: a deterministic generator locked to the
   references' own chunk order beats a vanilla run carrying scheduler jitter.)
   The earlier headline, 29,605 blocks and **99.972345%**, is kept in
   `docs/PARITY.md` as the record of what was published and is **not
   quotable**: it was taken with an **uncontrolled chunk order**, our dump
   walking chunks x-major against references harvested z-major, which the same
   instrument prices at 117,628 blocks on a candidate that is vanilla. Deltas
   against it remain exact. `docs/DETERMINISM.md` §15 and §16. The End over
   the same window is byte-identical to an unmodified vanilla run on three
   seeds. Sixteen of the twenty structure sets are still unbuilt and no block
   entities are written; `docs/PARITY.md` §6 is the list and every qualifier
   in it is load-bearing.

   **One r=16 overworld window does carry a quotable exactness figure, and it
   is seed 12345 rather than the one above.** Taken on a fresh eight-reference
   ensemble with our dump order matched to the references' own harvest order,
   that window reads **69,288 blocks of 107,053,056, 99.935277%**, beside an
   innocent-vanilla floor of 2,577 to 4,835 at the same N. It is the only r=16
   overworld figure that is quotable, the floor travels with it, and the floor
   is never subtracted. `docs/PARITY.md` §2.

   **Gameplay parity is a second instrument and a separate claim, and the
   README used to omit it entirely.** `ci/t2-gameplay.sh` drives **106
   scenarios** in `ci/checks/gameplay/` against a cold-booted **unmodified**
   vanilla 26.2 and scores each by comparing the two servers' recorded traces
   rather than against an expectation written here. A scenario that produced
   nothing to compare scores `vacuous`, `mute` or `inert` instead of passing:
   a check that cannot go red is worth nothing, and this project rediscovered
   that often enough to gate on it, so `ci/corpus-falsifiable.py` and
   `ci/liveness-proofs.py` run in T0 and refuse a check with no recorded
   red-when-broken evidence. **No corpus score is quotable as of 2026-08-14.**
   The comparator's projection changed, a floor belongs to the projection it
   was taken under, and every floor in this repo predates the change. Six cold
   vanilla runs re-take it; until they land, the score without its floor is not
   a measurement.
2. **Speed.** Targeted equally with parity, and expected to win on
   *published* numbers rather than on a favourable measurement setup.
   **Half a fact, and the half matters.** Re-measured 2026-08-03,
   `ci/bench-servers.sh` on one idle machine, two back-to-back runs pooled
   over six 225-chunk generation squares — seed 20260722, `BENCH_RADIUS=7`,
   `-Xms4G -Xmx4G` on all three JVMs. A finished chunk costs this server
   **less CPU than vanilla, Paper or Folia**: 0.082364 CPU-s against
   0.189644, 0.114578 and 0.130592. **Paper still wins the clock** — 4.61 s
   against our 6.55 s — and wall is the binding target. The first
   measurement, 2026-07-28 on the same square, had this server slowest of
   the four at 1.38x vanilla's CPU; it is superseded and kept in
   `docs/BENCH.md` as history. Boot and resident memory were first measured
   2026-08-05 and are no longer unmeasured: 15.0x faster to a completed
   login on superflat (n=12 each), and a memory result that went **against**
   us until it was fixed the same day. **Every figure in this bullet is
   r=7.** `docs/BENCH.md` also carries r=32 numbers; the two radii are not
   comparable and must never be quoted as a trend.
3. **The first-party plugin ecosystem.** The server and the plugins ship
   together, and the plugin layer is half the product rather than an
   accessory to it. A clean break from Bukkit/Paper is only survivable if
   the plugins admins actually use exist here on day one — so they are
   built, not imported. Native first-party crates; WASM sandbox for third
   parties.

Source-available, not open source: licensed under the PolyForm Noncommercial License 1.0.0 (see `LICENSE`). Free to use, change and share for any non-commercial purpose. Commercial use needs permission. Pull requests are merged only if you agree in writing that your contribution can be relicensed.

Start at `docs/VISION.md` (why) → `docs/PRODUCT.md` (what: the 12 decisions)
→ `docs/ORACLE.md` (how it's proven) → `docs/PHASE0.md` (current state).

## This repo needs `grit-plugins` checked out beside it

```
git clone https://github.com/PointOfPressure/grit-plugins.git ../plugins
```

`crates/rustmc-server` depends on `grit-plugin-api`, the plugin contract, and
that crate lives in the `grit-plugins` repo rather than in here. It is a path
dependency on a sibling checkout and **not a copy**: a mirrored crate would
make this server's plugin types a *different* crate from the one the
first-party plugins are built against, and no test anywhere could then load a
real plugin into the real server. `grit-plugins/docs/DECISIONS.md` D15 names
this direction as the correct one; it ends when `grit-plugin-api` is published
and the path becomes a version.

`ci/t0-remote.sh` ships the sibling to the build box along with the tree, so
no per-machine setup is needed. Set `GRIT_PLUGINS_DIR` if your checkout is
somewhere else.

## Build & gates

```
cargo build                  # workspace (toolchain pinned, rust-toolchain.toml)
ci/t0.sh                     # fmt + clippy + doc-tests
ci/t1.sh                     # unit tests
ci/t2-wire.sh                # wire harness vs REAL vanilla (needs java)
ci/t2-gameplay.sh            # 106-scenario gameplay corpus vs REAL vanilla
ci/t2-regiondiff.sh          # Anvil parser KAT + differ control + live harvest
ci/regiondiff-falsify.sh     # its red-when-broken evidence (4 injected faults)
ci/handoff.sh <branch>       # the merge gate: T0, T1, gameplay, T2, then main
```

`ci/handoff.sh` is the only route to `main`. It refuses a dirty or attached
worktree and a build directory another gate already owns, so two gates cannot
share a box; `ci/corpus-lock.sh` does the same for the vanilla reference boots,
which are exclusive because they are what everything is measured against.

It answers with one of eight words and one exit code — `MERGED` (0),
`ALREADY-MERGED` (0), `GREEN` (0, dry run), `RED` (1), `REFUSED` (2), `NOT-FF`
(3), `INFRA` (4), `UNPUSHED` (5). `RED` means the gate measured the branch and
the branch failed; `INFRA` means nothing about the branch was measured at all;
`UNPUSHED` means it passed and merged locally but `origin/main` does not have
it, and nothing else can be gated until they agree. The vocabulary is
prefix-free, every terminal state is declared in
`ci/checks/handoff-terminals.toml` with the control that forces it, and T0 fails
if the script drifts from the declaration. See `docs/CI.md`.

The oracle tools are self-contained: `ci/vanilla-server.sh fetch` downloads
the pinned vanilla server (sha1-verified) into `oracle-work/` (gitignored;
Mojang-derived files are never committed).

## Layout

- `crates/` — the server (interfaces FROZEN as of 2026-07-22, see
  docs/CRATES.md; `rustmc-api` is the public plugin surface)
- `plugins/` — first-party: essentials-core, permissions, worldedit-core
- `ci/checks/gameplay/` — the gameplay corpus, one `.toml` per scenario
- `tools/` — oracle-runner (wire client), region-differ, goldengen
- `swarm/` — worker harness + task files (docs/WORKERS.md)
- `docs/intel/` — the graveyard study + community mining behind PRODUCT.md
