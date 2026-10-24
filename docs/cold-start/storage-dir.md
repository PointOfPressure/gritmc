# Cold start — branch `storage-dir`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `storage-dir` off `plugin-hook` (`24c50ea`), 2026-08-07

**Item: `HostServices::storage_dir` — the seventh Tier 1 blocker. Done, and it
was one function.** Remote dir `~/rustmc-storagedir` on the **Threadripper**,
logs `~/grit/logs/storagedir-*.log`, worktree `~/grit/work-storagedir`. Not
merged — the orchestrator gates. **Merges after `plugin-hook`, which it is
built on.**

**What landed here (server):** `plugins::Host::storage_root` answers
`<world>/plugins`; `Tiles::configured_world_dir` exposes the operator's
`--world`; `tests/plugin_storage.rs` proves both halves and reads the plugin's
file back off the disk. `grit:storage.dir` is **derived** from `storage_root`
by the API rather than written into `capabilities()`, so it cannot be published
without being backed. **World protection now loads into the real server — the
first Tier 1 capability this server can back.**

**What landed in `grit-plugins` (pushed, `051672a`):** API 0.1 → **0.2**, four
breaking changes from `docs/DECISIONS.md` D24 — `storage_root` replacing
`storage_dir` + `Setup::require_storage_dir`; `Plugin::disable` **deleted**;
bus-owned plugin state with `&mut S` (`grit:protect` no longer locks a mutex on
every dig); and `Ctx::message`/`world`/`world_ref` returning
`Result<_, Refused>` so negotiation runs in both directions.

**Measurements, all on the Threadripper:**
* T0 GREEN, 0 `skipping: no vanilla`, on both `plugin-hook` (`24c50ea`) and
  this branch.
* Zero plugins: **+9.2 ns/dig against a 29.1 ns run-to-run spread** — not
  measurable (`ci/plugin-hook-cost.sh origin/main 5`, subject `24c50ea`).
  Consistent with `plugin-hook`'s own +1.9/45.2.
* Falsification **35 caught, 0 uncovered, 0 misattributed**
  (`grit-plugins/ci/falsify.sh`), plus 3/3 on the server's storage test.
* Plugins repo 82 tests, `host-proof` 13.

**Nulls — do not re-run these:**
* **Retaining plugins to call `Plugin::disable` was killed on a measurement.**
  This server has **no shutdown**: `status::listen` leaks the runtime to
  `&'static` and blocks on an accept loop with no exit, and `main.rs` says so
  ("Autosave, on a timer, and that is the whole shutdown story"). A
  `disable_all` would have had zero production callers — a fourth frozen
  interface. Bring it back **in the same change that adds a clean stop**, never
  before.
* **`grit:event.player_connect` was already corrected to "no" before this turn
  started** (by the `grit:protect` work). The brief asked for it; the tree had
  it. All three docs agree and no host publishes it.

**Two instrument failures worth more than the passes:**
* **`CONTRACT.md` §4 claimed "the day someone implements `storage_dir` [...]
  the test goes red". It did not.** `storage_root` landed and all 73 plugin
  tests stayed green — the plugins workspace has **no server dependency** (D15,
  deliberate), so `todays_server_capabilities()` is a list somebody typed and
  cannot observe the server. Found by implementing the thing it claimed to
  watch for and noticing nothing happened. The reconciliation now lives in
  `host-proof`, the only crate depending on both repos.
* **A falsification mutation went stale and the sweep printed GREEN**, which
  reads as an uncovered hole. Twice: once because `cargo fmt` split the line a
  pattern matched, once because this change edited the code three patterns
  matched. Both sweeps now abort a check whose mutation did not apply.

**Next item, and the result that closes it:** the **command registry** is now
the largest remaining blocker — it gates five of the seven Tier 1 capabilities
(permissions, player utilities, block log, region editor, economy) and there is
no other input channel from a player to a plugin. Closes when
`todays_server_admits_exactly_one_of_tier1` has to be renamed again because a
plugin requiring `grit:command.register` loads.
