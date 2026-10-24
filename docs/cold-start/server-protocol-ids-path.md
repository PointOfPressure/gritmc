# COLD START — branch `server-protocol-ids-path`

### COLD START — branch `server-protocol-ids-path`, 2026-08-12

**The item: `world-send-chunk-block-entities` (`4fddc9a2`, unmerged) reported
that `crate::protocol_ids::shared()` resolves its path relative to the
process's working directory, and `cargo test` runs with cwd
`crates/rustmc-server/` rather than the repo root — so `World::chunk` and
`broadcast::broadcast_block_entity_if_needed` cannot be exercised by any unit
test at all: a correct id and a wrong one both produce `None`. Make it
reachable regardless of cwd and land a test that proves the unlock.
`docs/PARITY-ROADMAP.md` §33.**

### What moved

```text
  protocol_ids.rs `shared()`   was: Path::new("data/vanilla/protocol_ids.json")
                                now: that, tried first — unchanged — then a
                                     CARGO_MANIFEST_DIR-anchored fallback,
                                     tried only when the first finds no file
  new test file                crates/rustmc-server/tests/
                                protocol_ids_reachable.rs — 2 tests
  ci/checks/test-targets.toml  new bucket protocol-ids-reachability,
                                1 member, runs_in = t1
  docs/PARITY-ROADMAP.md       §33 + docs/claims/PARITY-ROADMAP-33-
                                protocol-ids-path.toml
  T0 (ci/t0-remote.sh)         GREEN — fmt, clippy -D warnings, check,
                                787+/787+ workspace --lib --bins,
                                check-test-targets, check-doc-claims
```

### The fix, and which of the two named risks it carries

Brief named three approaches in order and two risks to check against. Took
approach 1 (`env!("CARGO_MANIFEST_DIR")`), narrowed to a **fallback** rather
than a replacement: the cwd-relative lookup runs first, unchanged, and the
compile-time-anchored one is only even computed when `Path::exists` says the
cwd one is absent.

* **"production path must keep working from the repo root exactly as it does
  now"** — holds by construction, not by measurement: the fallback branch is
  simply never reached on any checkout where the cwd lookup already succeeds,
  which is every real boot. No server was booted to check this because the
  `if`/`else` makes it structurally true rather than probabilistically true.
* **"the shipped binary must not start depending on the source tree's layout
  at runtime"** — this is the risk the fix **does** carry, narrowed rather
  than eliminated. `env!("CARGO_MANIFEST_DIR")` is a string baked into the
  binary at compile time; a release binary launched from the wrong working
  directory, on the same machine it was built on, would now silently read
  that build tree's table instead of refusing — the exact shape of the
  `data/vanilla` symlink incident `docs/AGENT-BRIEF.md` already tells this
  story about (a gate reading a foreign checkout's tables and reporting
  11/55 REGRESSED for a branch that was fine). On any other machine, or once
  the build tree is gone, the fallback path is simply absent and this refuses
  exactly as it does today, printing the same line it always did.

### Witnessed, not assumed

Reverted `shared()`'s body on the tree to the pre-fix single-path version,
re-ran `crates/rustmc-server/tests/protocol_ids_reachable.rs`, watched both
tests fail:

```text
protocol ids unavailable (No such file or directory (os error 2)) — no container or block event can be sent
test protocol_ids_shared_resolves_under_the_cargo_test_working_directory ... FAILED
test world_chunk_runs_end_to_end_beside_the_block_entity_store_and_its_ids ... FAILED
test result: FAILED. 0 passed; 2 failed
```

Restored the fix, re-ran, both green: `test result: ok. 2 passed; 0 failed`.

### What the new test actually exercises, and what it honestly does not

`World::chunk` on `main` still builds `block_entities: Vec::new()`
unconditionally — the branch that wires it from the store
(`docs/PARITY-ROADMAP.md` §32) is unmerged. So the new test does not, and
could not without changing behaviour this item is not scoped to change,
witness a *wrong* block-entity list coming out of `World::chunk` itself. What
it does:

* calls `crate::protocol_ids::shared()` — the exact zero-argument function
  that was unreachable — and cross-checks the id it returns for
  `minecraft:chest` against an independent read of
  `data/vanilla/protocol_ids.json`, so a wrong id (not just a missing table)
  would be caught;
* builds a real `World` (`tests/decorated_world.rs`'s own construction),
  places a chest through production `World::set_block`, and confirms
  `crate::blockentity::BlockEntities::in_chunk` — the function §32's `chunk()`
  wiring will call — actually holds it, over the real store, no fixture;
  a wrong list here would be caught too;
* calls `world.chunk(0, 0)` end to end and asserts `block_entities` stays
  empty **today**, with a comment naming this as the line §32 has to update
  when it lands — pinned rather than silently true.

### Nulls — killed with numbers, do not re-run these

* **"fix `World::chunk`'s `Vec::new()` here."** Out of scope by the brief:
  "this item is about reachability, not behaviour." Left for §32.
* **"replace cwd resolution with `CARGO_MANIFEST_DIR` outright."** Rejected:
  it satisfies "does not move any file" but fails "the shipped binary must not
  start depending on the source tree's layout at runtime" unconditionally
  rather than only in the narrow misconfigured-launch case the fallback form
  carries.
* **Option 2 (embed the data at compile time) and option 3 (env var
  override).** Not needed — option 1's fallback form clears both named risks
  well enough that the brief's own ordering was never revisited.

### The census: six other loaders share the exact fault, two more share the shape with extra parameters

Full table and citations: `docs/PARITY-ROADMAP.md` §33. Summary:

```text
  same zero-arg shared()/load_default() shape, bare cwd literal, no override:
    entities::shared_kind_ids      entities.rs:751     entity_types.json      — LIVE, reachable from world_send.rs/status.rs
    power::shared                  power.rs:194         signal.json           — LIVE, reachable via shapes.rs set_block
    attributes::Attributes::shared attributes.rs:364     attributes.json
    movement_props::…::shared      movement_props.rs:276 movement_props.json  — currently uncalled
    ops::shared                    ops.rs:119            ops.json (not data/vanilla)
    collide::…::load_default       collide.rs:594         collision_shapes.json — currently uncalled by anything

  same bare-literal shape, extra params (seed/radius), boot-path only:
    world::World::prepare / tiles::Tiles::prepare   world.rs:310-312, tiles.rs:281-283

  same shape, separate binary (tools/oracle-runner):
    main.rs:106, compare.rs:1030
```

Not counted: `session_tables.rs`, `shapes.rs`'s `shared_with_classes`, and
every `shared(path: &Path)`/`load(path: &Path)` loader that already takes a
path (`placedby`, `useblock`, `recipe_book`, `sound_types`) — those are
correctly parameterized already; the bare literals at their production call
sites in `status.rs`/`main.rs` are the same one-call-one-literal shape
`session_tables.rs`'s own doc names as correct, not the fault.

`entities::shared_kind_ids` and `power::shared` are worth flagging loudest:
both are in production call chains a unit test can already reach today, so
both are silently returning empty/`None` under `cargo test` right now,
exactly as `protocol_ids::shared()` was before this branch.

### The single next item, and the result that closes it

Not this branch's to take: **fix `entities::shared_kind_ids` and
`power::shared`** the same way, since both are live and already reachable
from code a unit test calls. `movement_props`, `attributes`, `ops` and
`collide::load_default` are lower priority — the first three are correct in
production and simply untested; the last is dead code.
