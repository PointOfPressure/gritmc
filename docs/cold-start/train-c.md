# COLD START — branch `train-c`

Append-only. Newest entry last.

### COLD START — branch `train-c`, 2026-09-30, the CI-stack and engine landing train

Worktree `~/grit/work-train-c`, remote dir `~/rustmc-train-c`, scripts in the
lane's scratchpad (`tc/`). Built from origin/main 3745 (`c52d52e7b`), each
branch's own commits rebased or cherry-picked in this order, with compile and
clippy (`--locked --all-targets -D warnings`) after each:

1. `stack-ci` (233 commits, `git rebase origin/main`, four hand conflicts:
   main.rs `--autosave-seconds` kept main's tick semantics under stack-ci's
   refuse-on-unparseable `flag`; handoff.sh verdict lines kept main's
   persistence arm and dropped "0 skips"; stage timing marks stage 7; ten
   figures provenances took main's side). Its two gate reds fixed for real:
   a `||||||| base` line and ten duplicated lines in FINDINGS.md, and the bare
   citation ceiling (114 was one short of its own tree). Plus a Cargo.lock
   entry 66d24e128 never wrote.
2. `grepq-pipefail`: 65 more `| grep -q` sites, and the checker's greedy
   regex hid three more; fixed.
3. `portability`: its cross-check found main's `shutdown.rs` does not build
   for Windows; fixed with a cfg'd cast.
4. `vacuous-green-audit`: 8cf8f84d6's snapshot route dropped in favour of
   stack-ci's object store; its empty-ledger refusal ported. Census
   re-derived, 13 new instances declared, 3 retired, 1 dead handler repaired.
5. `light-engine-2`: the T0 red was real — the light arrays were read and
   dropped. Now digested (`sky_data`/`block_data`) with a one-nibble test.
6. `chunk-streaming`, 7. `entity-tracker`: argv parsing kept stack-ci's
   refusal; status.rs join sequence kept main's rule-driven fields beside the
   branch's simulation distance and chunk view.

Ratchets set to what the tree measures, reason beside each:
`REQUIRED_FLOOR` 25, `MAX_BARE_CITATIONS` 120, vacuous-green ceilings
30/11/12, `FIELD_UNIVERSE` 110, t1-controls 113 binaries (92 demonstrated,
9 instrument, 4 unforceable, 2 declared red).

Full corpus in `~/rustmc-train-c`: 152/152 both sides, T2-GAMEPLAY GREEN;
sweep tier 2 152/152, dead fields 4/110.

**Nulls, so nobody re-runs them:** the nine `red-before-forcing` grades in
the first t1-controls pass were a missing `data/vanilla/enchantments.json`
(tables not yet generated), not the binaries.

**Next:** the corpus floor (`ci/checks/gameplay-floor.toml`) is stale on
projection and denominator (5/108 under `proj:1277e43a…`; this run compares
under `proj:7f832163…` over 152). `ci/corpus-floor.sh` over two or more
recordings taken at this projection re-measures it.
