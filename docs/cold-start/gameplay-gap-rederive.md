# Cold start — branch `gameplay-gap-rederive`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gameplay-gap-rederive`, off `main` (`7ccad044`), 2026-08-11

**Docs only. No server code, no corpus run, no corpus verdict, no lock taken.**
Worktree `~/grit/work-gapderive`. Not merged — the orchestrator gates. Full
account in `docs/PARITY-ROADMAP.md` **§19**; §4, §5, §6 and §7 were stamped
with status in place.

**THE NUMBERS.** `~/rustmc-water/oracle-work/gameplay/report.json`, recorded
2026-08-10 21:31/21:49, corpus 88, seed 20260803, Threadripper:
**declared 64/88, strict 60/88, floor 7/88** — corroborated on four further
independent runs the same day (`-sulfur2`, `-sulfur`, `-grassdirt2`,
`-w22perblock`), all five identical. `~/rustmc-iceberg` 17:46 reports 26/88 and
is **excluded**: the subject did not come up in that run. `0 vacuous, 0 inert,
0 error` on both denominators.

**THE POPULATION SPLIT, which is the deliverable.** 28 scenarios are non-`match`
on at least one denominator: **17 our defect, 11 cannot convert, 0 instrument
defect.** Seven of the eleven are the standing floor exactly; the other four
(`050`, `530`, `605`, `606`) are strict-only and are established in §10/§11/§16
because **no mechanised strict floor exists** (§16.5: one `--projection strict`
flag on `measure`).

**THE NEXT THREE, cheapest real conversion first — §19.3 has the full rows.**

1. **The sneak flag.** 0 packets, converts `609` + `552`. `PLAYER_INPUT` (43)
   is declared in `rustmc-proto` (`ids.rs::PLAYER_INPUT` (`:201`), `FLAG_SHIFT = 32`) and read
   **nowhere** in `crates/rustmc-server`; `PlayerState` (`player.rs:371`) has
   no crouch field; the gate goes at `world_send.rs:1212`. Negative control
   **already exists**: `553`, `match`/`match`, the same rig without the sneak.
2. **The comparator use sound.** 1 packet, converts `420`. Third arm in
   `world_send.rs::use_sound` (`:7465`), which today has two of 184. Verified
   from the traces, not inferred: every `block_update` is byte-identical and
   the *whole* difference is vanilla's `sound 383` at volume 0.3. Controls
   `290`/`330` (added, not replaced) and `470`/`480` (not over-emitted).
3. **Item-aware `canBeReplaced`.** 0 packets, converts `250` + `170`.
   `world_send.rs::handle_use_item_on` (`:5077`), at `:1927` and `:1945`, via
   `world.rs::replaceable` (`:1581`), a static boolean
   where vanilla's reads the held item. Controls `120` and `020`; the tell is
   that `world_send.rs:2289`'s test pins the current behaviour and **must** be
   edited.

**RANK 0, above all three and converting nothing:** no test in this repo ever
builds a `Session` with `placement`/`shapes`/`sound_types`/`ticks`/`placed_by`/
`recipes` set, so the connection-loop wiring is witnessed only by a 50-minute
locked corpus run. §19.4.

**FOUR THINGS THAT WERE BELIEVED AND ARE NOT TRUE.** Each is written up with
its receipt.

* **The Session finding is 6 sites, not 5** — five in tests plus **one in
  production** (`status.rs:989`), where all six fields are `Some`. So it is
  *one untested joint six times over*, not six untested mechanisms. The
  distinction is the whole item.
* **§4's "neighbour-update counts" family is refuted.**
  `power.rs::update_neighbors_at` is a faithful port; `250`/`170` are
  `canBeReplaced`, `310`/`320` are the absent `Item.useOn` layer, `440` is
  unattributed.
* **`607` is a check that cannot fail and it is passing.** Its measured
  `pickup_all` click produces **zero observations on both sides** while
  `click.rs:740` documents the arm as unimplemented. Filed as a finding, the
  scenario deliberately **not** edited.
* **§7's four `kind = "corpus"` figures are unregistrable.** The schema was
  never adopted, and no gameplay recording before 2026-08-08 21:26 survives on
  the box — `subset/vanilla.json` exists in zero trees.

**NULLS — do not re-run.** No remaining scenario is blocked on the instrument
(R1-R3 landed `20b41e34`, `4eb45f51` closed `540`'s silence). Do not open an
item on `update_neighbors_at`. Do not price the other 182 use sounds off `420`.
Do not read `080`'s `match` as entity-movement coverage. Do not re-derive §7's
figures.

**UNKNOWN, said rather than guessed:** `400`, `440` and `410`'s mechanisms;
whether `430`'s `block_event` is reachable before its `basedrum`/`harp`
placement defect; and the tip behind the 64/60 figure (`.prov` says
`commit=unknown`).

**HAZARD FOR WHOEVER MERGES.** `0878cc4d` and `4b532a71` **each add a different
`## 18.`** to `docs/PARITY-ROADMAP.md`. This branch took **19** to avoid making
it three-way. One of those two must be renumbered, and `grep -n '§1[89]'` over
`docs/` before it lands.
