# Three workers, three worktrees

Three long-lived Opus 5 workers run alongside the orchestrator. They are
*workers*, not one-shot tasks: each is spawned once and given follow-up work
by message, so their findings survive between assignments and nothing is
re-derived.

## Why worktrees, not one directory

Three agents editing `~/rustmc` at once corrupt each other's builds — this
project has been bitten by it before. Each worker gets its own `git worktree`
and its own branch:

```sh
git worktree add -B worker-a /home/sayer/rustmc-wa main   # and -wb, -wc
```

`data/vanilla/` and `oracle-work/` are **gitignored**, so a fresh worktree
has neither, and both are large (25 MB and 11 MB a world). They are
symlinked rather than copied:

```sh
ln -sfn /home/sayer/rustmc/data/vanilla   /home/sayer/rustmc-wX/data/vanilla
ln -sfn /home/sayer/rustmc/oracle-work    /home/sayer/rustmc-wX/oracle-work
```

Workers read those and never write them. A worker that regenerates the
oracle would corrupt every other worker's measurements at once.

## The split

Ownership is by **fault cluster**, chosen so the files barely overlap:

| worker | tree | area | size when assigned |
|---|---|---|---|
| A | `rustmc-wa` | mineshaft block placement (`swarm/tasks/w11`) | ~380 blocks |
| B | `rustmc-wb` | aquatic vegetation — seagrass, kelp, tall seagrass | ~585 blocks |
| C | `rustmc-wc` | sulfur caves — `sulfur_pool`, spikes, clusters | ~860 blocks |

The orchestrator keeps everything that needs the whole picture: the r7
measurements, the oracle-soundness questions, `docs/BENCH.md`, and merging.

## What every worker is told

The parts that are not obvious and that a worker will get wrong without
being told:

- Read the reference from the **jar's bytecode**, never transcribe it.
- Refuse loudly rather than approximate; a refusal is counted, a guess is
  invisible.
- Draw counts are behaviour — a condition that gates a `random.next*()` call
  moves everything after it.
- **`DecorateGolden` is not sound for every feature.** It builds its own 3x3
  world, so for anything placed through `environment_scan` or a
  `block_predicate_filter` its world is not a server's. This port has already
  been shown to match the harness exactly while both differ from the real
  world. Trust it for positions, draws and the pre-decoration world; report a
  disagreement about final blocks rather than "fixing" toward it.
- Never report a fix without a before/after block count on a named chunk.

## Cost

Three concurrent workers multiply token consumption. A single agent running
continuously stays inside the subscription; three will not necessarily. Worth
watching rather than assuming.
