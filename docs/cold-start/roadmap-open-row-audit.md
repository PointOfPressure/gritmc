# Cold start — branch `roadmap-open-row-audit`

Every `### COLD START` entry ever written for this branch, newest last.

---

### COLD START — branch `roadmap-open-row-audit` off `b94f3ebf`, 2026-08-17

**The area was: which OPEN rows in `docs/PARITY-ROADMAP.md` are still open.**
Answer: fewer than half of them. Full reconciliation is **§52**, written from
recordings that already existed — **no corpus run, no lock, no server boot, and
none was needed.** Nothing in this branch touches source.

    24 scenarios carrying a §4 or §0 claim, checked against 4 recordings

    STALE — row said open, evidence says closed        8 scenarios
    STALE THE OTHER WAY — a closure that regressed     0
    ROW GRADES A SCENARIO THAT DOES NOT EXIST          1  (160)
    ROWS THAT DUPLICATE ANOTHER ROW                    2  (both sound headings)
    LIVE DIVERGENCES WITH NO ROW AT ALL                2  (780, 790)

#### The instrument

Four independent full-corpus recordings, all 2026-08-17, all `n = 108`, all
**agreeing scenario for scenario with an empty disagreement set**:
`~/rustmc-stack` 20:46, `~/rustmc-refcache-bw-A` 20:02,
`~/rustmc-refcache-bw-D` 18:36, `/home/sayer/grit/work-itemfix` 12:31. Each
reads declared 97/8/3 and strict 93/14/1 of 108.

Three controls, and **the first is the one that makes the rest mean anything**:

* **Coverage** — 108 check files against 108 recorded scenarios, symmetric
  difference empty both ways. A scenario that is not recorded produces no row
  and reads exactly like one that passed. Check this first in any repeat.
* **Falsifiability** — 108/108 tier 2 in `ci/checks/gameplay-falsifiable.toml`.
* **Stability** — 14 full-corpus reports, 2026-08-12 23:22 to 2026-08-17 20:46,
  zero flaps among the 24 audited.

#### What was closed on existing evidence

604, 607, 550, 551, 609, 552, 420, and the 250/310/320/440 family. The fixes
landed in `3428600a`, `96d2bf84`, `de3f2e3f` and `6783d819` — the last of which
is dated **2026-08-10, one day before the row asserting the defect was
written**. The oldest false gap ran six days.

#### Nulls — do not re-run these

* **Do not go looking for a regression.** Every row claiming a closure still
  holds. §0's five carried-forward verdicts were re-checked specifically for
  this and all five are `match`/`match` fourteen reports running.
* **Do not start piston work off the "random pitch" row.** It is the
  block-events row wearing a sound heading; 380 and 390 never report `sound`.
  Starting from it prices the same piston twice.
* **Do not start a 604 `SWAP` port, a 607 `PICKUP_ALL` port, a combat port, or
  a sneak-gate port.** All four are in `main` and all four are green.
* **Do not treat 160 as a fluids item with a failing scenario.** There is no
  scenario. `water-spreads` was parked out of the corpus by `e47d7ca3`.

#### The five greens that are narrower than they look — read before closing anything else

1. **`sound.pitch` is read and discarded** — `trace.rs:1498`,
   `let _pitch = read_f32(&mut r).ok()?;`, pinned deliberate by the test at
   `:1977`, and **not** folded into `rest`. No check in this corpus can grade a
   pitch. 420's green covers event id, position, source and volume and says
   nothing about the `pitch 0.55` its row is about.
2. **550/551 observe `damage_event`, `set_health`, `entity_event` and vanilla
   emits none of them** — a bare-stick attack deals no damage. The green is
   about the sound and the reach test. **Damage is not covered by anything.**
3. **607's green only became informative on 2026-08-13** — same check, same
   verdict, projection grew 12 → 17 observations when the recorder learned to
   decode the item stack. A green dated earlier is not evidence.
4. **608 is tier 2 via `speak`** — its content is an assertion of absence.
5. **611's raw traces differ (`state` 7 vs 5) and that is correct** —
   deliberately rebased at `compare.rs:598`.

#### What is genuinely open, with the shape of each gap

| scen | verdict | missing | shape |
|---|---|---|---|
| 380 | `diverge`/`diverge` 7→4 | `block_event` | behaviour — no piston |
| 390 | `diverge`/`diverge` 7→4 | `block_event`, `section_blocks_update` | behaviour — same piston |
| 430 | `diverge`/`diverge` 6→4 | `block_event`, `sound` | behaviour + extraction |
| 400 | `diverge`/`diverge` 5→4 | **none** | behaviour, content not coverage |
| 070 | `diverge`/`diverge` 10→2 | `add_entity` | behaviour — falling sand |
| 790 | `diverge`/`diverge` 5→3 | `container_set_slot` | behaviour — `useWithoutItem` |
| 780 | `absent`/`absent` 2→0 | `block_update` | behaviour — random tick |

#### The single next item

**Merge `corpus-sound-pitch`.** It is the only blocker here that is an
*instrument* rather than a port: until it lands, both §4 sound headings are
ungradeable in either direction, and 420 cannot be honestly called closed. The
result that closes it is a recording in which a `sound` observation carries a
pitch field and 420 is re-read against it.

After that, the one large open family is **block events** — 380, 390, 430, all
three behind the same unported piston plus a note block, and all three
correctly described by that row.
