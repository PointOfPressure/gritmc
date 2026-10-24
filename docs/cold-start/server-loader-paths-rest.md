# COLD START — branch `server-loader-paths-rest`

### COLD START — branch `server-loader-paths-rest`, 2026-08-12

**The item: `server-protocol-ids-path` (`37f2ba42` on this branch, since
landed on `main` as `docs/PARITY-ROADMAP.md` §33) fixed
`protocol_ids::shared()`'s cwd-relative path bug and counted six more
loaders sharing the exact fault. This branch
fixes five of them, deletes the sixth, and answers the question the census
did not: did any existing test's assertions actually measure a `None`
branch as a pass. `docs/PARITY-ROADMAP.md` §34.**

### What moved

```text
  entities.rs  shared_kind_ids()          cwd-then-CARGO_MANIFEST_DIR fallback
  power.rs     shared()                   same
  attributes.rs Attributes::load_default() same, via existing load(path)
  movement_props.rs MovementProps::load_default() same, via existing load(path)
  ops.rs       shared()                   same (ops.json, not data/vanilla)
  collide.rs   CollisionShapes::load_default() DELETED — unused, load(path) stays
  new test file  crates/rustmc-server/tests/loader_paths_reachable.rs — 5 tests
  ci/checks/test-targets.toml   new bucket loader-paths-reachability, 1 member
  docs/PARITY-ROADMAP.md   §34 + docs/claims/PARITY-ROADMAP-34-loader-paths-rest.toml
  T0 (ci/t0-remote.sh)     GREEN — fmt, clippy -D warnings, check, 558/558
                           workspace --lib --bins, check-test-targets, check-doc-claims
```

### The fix

Copied `protocol_ids::shared()`'s shape exactly, five times, no variation:
`Path::exists` on the cwd-relative candidate decides whether to try a
`CARGO_MANIFEST_DIR`-anchored one. `attributes`/`movement_props` already had
a parameterized `load(path)` underneath their `load_default()`, so the fix
there is just resolving the path before the existing call rather than a new
load routine. `ops::shared()` is not a `data/vanilla` table (`ops.json` sits
beside a real deployed server's jar) but gets the identical two-tier
resolution anyway — same fault, same fix, regardless of what the file is.

### Witnessed, not assumed

All five bodies reverted in place to their pre-fix single-path form,
`cargo test -p rustmc-server --test loader_paths_reachable` re-run:

```text
test result: FAILED. 0 passed; 5 failed
  attributes / entities / movement_props / power: unreachable, .expect fires
  ops: assertion left == right failed — left: All, right: Admins
    (the fixture at the repo-root fallback was never seen)
```

Restored, `test result: ok. 5 passed; 0 failed`. The `ops` test is the one
worth reading closely: it writes a real fixture at the
`CARGO_MANIFEST_DIR`-anchored fallback location (the repo root of this
worktree), refuses instead of writing if a real `ops.json` is already
there, and cleans up with a `Drop` guard that ran correctly through all five
panics above.

### `collide::CollisionShapes::load_default` — deleted

Zero callers anywhere, production or test — confirmed by grep, not assumed.
`CollisionShapes::load(path)` next to it is not dead: `tests/entity_move.rs`
calls it four times against its own `CARGO_MANIFEST_DIR`-anchored path, the
already-correct convention. Fixing a function nothing calls would mean
writing a witness test for a fix with no consequence; deleting it removes
the second, redundant, unfixed entry point that was the actual trap.
`TABLE_PATH` stays (still documents the canonical location; the literal
`ci/gen-gameplay-tables.sh` needs is still matched, doubly, via `TABLE_PATH`
and `entity_move.rs`'s own copy).

### The headline finding

**No existing test's assertions change meaning now that any of the six
loaders actually load.** But the six do not fail the same way:

* `entities::shared_kind_ids`, `attributes::Attributes::shared`,
  `ops::shared` (via `ops::level_for`): **genuinely unreached** by any
  existing test. Their only production callers sit behind a `DropItem`
  player action (id 4 — every existing test that sends `PLAYER_ACTION`
  sends action id 0, `StartDestroyBlock`), a full-inventory
  `container_click` drop, or `status::listen`'s join sequence, which no
  test drives (the one test that runs `status::serve_one` deliberately
  disconnects in the *configuration* state, before play). `movement_props::
  MovementProps::shared` is stronger still: **no caller at all**, test or
  production.
* `power::shared` is the interesting one: it **is** called by dozens of
  existing tests, on every `crate::shapes::set_block`, and returns `None`
  in all of them today — and still nothing notices, for three independent,
  checked reasons (full detail in §34): `power.rs`'s own redstone tests
  bypass the global and inject their own table; the two `set_block`-driven
  wire tests in `shapes.rs` assert only the shape cascade, a structurally
  separate branch that never reads `power::shared()`; and every other
  `set_block`-driving test names a block `power::execute_update`'s
  three-way dispatch (wire / `redstone_torch` / `redstone_lamp`) does not
  recognise — including `session_joint.rs`'s `minecraft:torch`, which is
  the light block, not the redstone one.

No behavioural bug was found in the redstone path while establishing this.
Not looked for beyond what answering the question needed.

### Nulls — killed with numbers, do not re-run these

* **"`power::shared` needs the same urgency treatment as a genuinely-tested
  loader."** It doesn't: reached on every write, observed by none, for
  reasons specific to how the existing redstone tests are built (inject
  directly) rather than to this loader being safe.
* **Fixing `collide::CollisionShapes::load_default` instead of deleting
  it.** Rejected — see above.
* **A behavioural audit of the redstone `set_block` path**, beyond what was
  needed to answer whether any assertion depends on `power::shared()`. Out
  of scope by the brief: this item is reachability, not behaviour.

### The single next item

Not this branch's to take: wire up `crate::collide::CollisionShapes` into
the boot path (nothing in production calls `physics.rs`'s collision
functions with a real table yet — `physics.rs`'s own tests use a hand-built
fixture). When that caller is written, it should resolve its own root the
way `entity_move.rs` and `session_tables.rs` already do, not reach for a
zero-argument convenience wrapper.
