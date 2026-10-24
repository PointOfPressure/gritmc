# Cold start — branch `ci-readiness-proofs`

---

### COLD START — branch `ci-readiness-proofs` off `ci-unfalsifiable-root`, 2026-08-13

**`ci/liveness-proofs.py --check` reads 5/5, 0 pinned unsound, `MAX_UNSOUND`
0.** It read 2/5 with 3 pinned when this branch started. Remote dir
`~/rustmc-ready` on the Threadripper. Not merged — the orchestrator gates.

**Based on `ci-unfalsifiable-root`, not on `main`.** The item's demonstration
*is* `ci/liveness-proofs.py`, which lives only on that branch, so this one is
its five commits plus mine. If that branch merges first this rebases to
nothing; if it does not, gating this one gates both.

**What shipped**

* `crates/rustmc-server/src/status.rs` — `listen` takes `on_bound` and calls
  it after the bind, after the tick loop, immediately before the first
  `accept`, with `listener.local_addr()`. Zero calls on any `Err` path, and
  that is a unit test rather than a fact about where a line sits in a file.
  `main.rs` announces from the callback. The `joined at` line moved after
  `enter_world` for the same reason.
* `ci/server-ready.sh` — `ready_socket_owner` (`ss -ltnpH 'sport = :N'`, pid
  match) and `ready_speaks_status` (the recorder's own client stack). Both
  `exit 3` on a missing tool rather than returning false.
* The three sites, each with its own arm set and its own reason —
  `docs/PARITY-ROADMAP.md` §28 has the table. All three now refuse on
  timeout instead of falling through and recording anyway.
* `ci/readiness-falsify.sh`, in T0 — boots the real binary into four
  not-ready states and runs each script's readiness line *lifted verbatim out
  of the registry* against every capture. 22.1 s wall, 6.2 CPU-seconds, warm.
* `ci/checks/liveness-proofs.toml` rewritten: three distinct negatives, one
  per shape. `ci/liveness-proofs.py`'s selftest no longer derives its
  expectations from `MAX_UNSOUND`.

**Live findings**

* **The brief's "make a failed bind exit non-zero" was already true**, since
  `fd1527e0` on 2026-07-26. The status was always 1; nobody read it, because a
  louder wrong signal had already been read. Adding a signal to a system
  reading the wrong one does nothing — deleting the wrong one is the fix.
* **`ci/liveness-proofs.py` alone can no longer catch a server-side
  regression, and this is the load-bearing consequence of the repair.** With
  the announcement past the bind, `grep -q 'listening on'` is *sound*, so the
  registry's negative fixtures contain no such line. Put the early print back
  and every fixture in that file becomes fiction while the gate stays green.
  `ci/readiness-falsify.sh` is the only thing that boots a server, and it is
  in T0 for exactly that reason. **Do not remove it as redundant.**
* **The gate refused its own author.** The first `ci/readiness-falsify.sh`
  waited for its foreign server by grepping a log; `--discover` found it and
  refused it as unregistered. It now waits on the two pid-attributed arms.
* **A fourth defect at all three sites:** the 60-iteration loops fell through
  and recorded anyway. A merely slow server — `Tiles::prepare` builds a spawn
  tile before the bind — produced a run whose only symptom was whatever
  `record` said about a refused connection.

**Nulls — do not re-run these**

* **`ci/t2-vanilla-loads-ours.sh`'s `sleep 5` calls are not a fourth
  unfalsifiable readiness wait.** It boots through `ci/vanilla-server.sh`,
  whose `Done (` proof is sound, and the sleeps wait on console commands whose
  delivery is separately proved by a trailing `ALL_QUERIES_DELIVERED` marker.
  Looked exactly like a hole from `--discover`'s blind spot; it is not one.
* **Every other print in the startup path is honest.** Audited: `spawn tile:`
  (after `Tiles::prepare`, which blocks on `generate`), `generating tile …`
  (present tense), `autosave: wrote N`, `plugins: N loaded`, both
  `--level-type` refusals. Only `listening on` and `joined at` claimed
  success early.
* **Widening `--discover` to catch `sleep`-based waits is not worth it.** It
  keys on `grep -q` because that is the shape a *log* proof takes; a sweep
  over every `sleep` in `ci/*.sh` is dozens of false positives, and the one
  candidate it would have found was the null above.

**Not done, and deliberately**

`ci/t2-gameplay.sh` was not run end to end. It needs the box-wide corpus lock
for forty minutes and several trees are contending; the readiness lines it
ships are run verbatim against a real server in five states by
`ci/readiness-falsify.sh`, which is a stronger test of the thing that changed
than one corpus run would be. **The next agent to hold that lock for any
reason should confirm the run reaches `recorded 1/86`** — that is the only
part of this change no instrument here has exercised in context.

**The single next item, and the result that closes it**

**`ci/t2-gameplay.sh` is the only readiness proof still without a handshake
arm, and the reason is a price nobody has paid rather than a decision.** Run
one full corpus with `ready_speaks_status` added to its line and one without,
and compare the two `report.json` verdict tables. Closes when the two are
identical — at which point the arm goes in and all three sites agree — or when
they differ, which would be a much more interesting finding: a status ping
before recording that changes what the corpus measures.
