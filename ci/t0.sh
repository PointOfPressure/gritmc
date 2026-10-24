#!/usr/bin/env bash
# T0 — fmt + clippy + check (CI.md tier table). Agent-local, < 1 min.
# Workers run this before every push; a red T0 push is a playbook violation.
set -euo pipefail
cd "$(dirname "$0")/.."
# The cheap block's failure counter (see "the cheap block reports together"
# further down). Defined first, on train-c, so the keep-going falsifier main
# runs before everything else reports through it rather than a bare exit 1,
# which ci/t0-ordering-falsify.sh refuses in the cheap block.
T0_STAGES_FAILED=0
t0_stage_failed() { T0_STAGES_FAILED=$((T0_STAGES_FAILED + 1)); return 0; }
# Stage timing (always) and T0_KEEP_GOING=1 (every stage runs, all reds listed,
# non-zero if any). The whole mechanism and its caveats: ci/t0-stages.sh.
. ci/t0-stages.sh

# ...and the mechanism above is itself a check, so its red-when-broken evidence
# runs before anything it would time or keep going through: ten red shapes this
# file uses, each alone and all at once, default mode byte-identical to plain
# bash, the real verdict tail, and seven broken copies of the library that must
# each be rejected. ~9 s wall, no build.
bash ci/t0-keepgoing-falsify.sh --quiet \
    || { echo "T0 RED — ci/t0-keepgoing-falsify.sh: keep-going or stage timing can lie" >&2 || true; t0_stage_failed; }


# PREFETCH (ci/t0-stages.sh section 3). These eight start now, each in its own
# snapshot of the tree, and are judged by `t0_prefetched` at their usual place
# further down, with their usual redirections and verdicts. Each qualifies the
# same way: it needs neither target/ nor ../plugins, and it either only reads
# the tree and the pinned jar (the two --check readers, click-surface) or
# edits files and restores them (the two -falsify scripts beside them),
# which in a snapshot can no longer be seen by any other check mid-edit, or
# builds its own scratch trees (corpus-exclusion, reference-cache). Measured
# serial on main 606551b54 (~/rustmc-gpipe, warm): the stages holding them are
# 26+30+15+34+31 s. A command changed at its call site but not here simply
# runs inline there, as before — the list only ever buys time.
# NOT here, measured: ci/entity-broadcast-order.py --check runs `cargo test`
# for our walk, so in a snapshot it has no target/ and REFUSED ("the walk dump
# did not run") in this branch's own T0 on 2026-09-30 — loudly, which is the
# failure a job needing the build is supposed to have. It and its falsifier run
# inline.
t0_spawn ./ci/reference-cache-falsify.sh
t0_spawn bash ci/t1-par-falsify.sh
t0_spawn ./ci/corpus-exclusion-falsify.sh
t0_spawn python3 ci/entity-id-allocator.py --check
t0_spawn ./ci/entity-id-allocator-falsify.sh
t0_spawn python3 ci/order-keyed-census.py --check
t0_spawn ./ci/order-keyed-census-falsify.sh
t0_spawn python3 ci/click-surface-falsify.py

# # THE SKIP LEDGER. 2026-08-18.
#
# A stage that cannot run in this environment is not a stage that failed, and
# until today the two were the same red. `ci/lib/box_env.py` makes the first
# kind print one `BOX-SKIP` line; this collects them and the audit at the
# bottom of this file reds on any that nothing declared.
#
# It is a FILE and not just stdout because thirteen stages below filter their
# own output — four through a fixed `grep -E` pattern list, nine through
# `tail -1` or `tail -2` — so a marker printed mid-run reaches no log at all.
# That is how the first version of this repair produced a skip that was
# correct, printed, and invisible. Exported so every child process appends to
# the same ledger.
export RUSTMC_BOX_SKIP_LOG="${RUSTMC_BOX_SKIP_LOG:-/tmp/t0-box-skips.$$.log}"
: > "$RUSTMC_BOX_SKIP_LOG"

# # ONE ENVIRONMENT FAULT AT LINE 66 HID EVERY STAGE BELOW IT, FOR A DAY.
# # 2026-08-18.
#
# `ci/check-figures.py` reddened in the `.git`-less copy every gate runs in, it
# is the third stage in this file, and this script exited on it. So no stage
# below ran remotely at all: the merge train died at 09:12:50 and every branch
# after it reported `t0=1/0` with T1 and T2 at 99, never reached. The check was
# wrong for a day; the ORDERING is what made that cost a day.
#
# So the cheap stages now run to completion and report together, and only the
# BUILD stages keep first-failure. The split is on measured cost, not taste:
#
#     the pre-cargo block          ~166 s      (stage timings, 2026-08-17)
#     T0 GREEN                     65.2 min    (N=164)
#     T0 RED                       3.4 min     (N=164)
#
# The worst case this adds is the whole cheap block on a red that used to stop
# at the first stage — 166 s against a 65-minute green, and against the day a
# masked red already cost. Below `cargo fmt` the arithmetic reverses: those
# stages are minutes each and a clippy red makes the test stages meaningless,
# so continuing there would buy noise with real time.
#
# `t0_stage_failed` returns 0 deliberately. Every call site is inside a
# `|| { ... }` group, so the group must succeed or `set -e` ends the run and
# the change would have accomplished nothing.
# (`T0_STAGES_FAILED` and `t0_stage_failed` are defined at the top of this
# file, so the keep-going falsifier that runs first can use them too.)


# MERGE-REGION BEGIN checks check=bash-syntax
# **Every shell script in the tree has to parse.** DEFECT CLASS #21, found
# 2026-08-06: `ci/t2-memory.sh` had not parsed since the commit that
# introduced it — `"tile radius ${RADIUS:-<the server's own default>}"`, where
# the apostrophe opens a quote inside the `${var:-word}` default — so the
# memory guard `docs/PRODUCT.md` calls "CI-enforced" had never run once, and
# the commit that broke it quotes a number in its own message.
#
# A guard that cannot fail is the class this project has now found twenty-one
# times, and this was the first instance in which the instrument did not
# **start**. That is worse than a check that cannot fail, because a check that
# cannot fail still emits a verdict somebody reads, and a script that aborts
# on a parse error emits nothing that looks different from not being run.
#
# `bash -n` is a parse, not an execution: it costs milliseconds for the whole
# tree and it is the only thing that catches this. It is first in T0 because
# it is the cheapest and because a tree whose gates do not parse has nothing
# to say about its code.
#
# It does **not** catch a script that parses and misbehaves — `shellcheck` is
# the tool for that and is not in this toolchain. Stated so the pass is not
# read as more than it is.
#
# **The sweep needs a floor, for the same reason `min_binaries` exists.**
# `find` runs inside a process substitution, so its exit status reaches nobody
# and `set -e` cannot see it; a selector that stops matching — a renamed
# directory, one more `-not -path`, a `find` that errors — produces an empty
# loop, `SHELL_BAD=0`, and a pass that is byte-identical to the pass a healthy
# sweep prints. That is this project's own defect class living inside the guard
# that was written to catch it. The floor is the measured count on 2026-08-07
# (86), less a small margin for a branch that legitimately deletes a script;
# it exists to catch the number **collapsing**, not drifting.
SHELL_BAD=0
SHELL_N=0
while IFS= read -r script; do
    SHELL_N=$((SHELL_N + 1))
    bash -n "$script" || { echo "T0: $script does not parse" >&2; SHELL_BAD=1; }
done < <(find . -name '*.sh' -not -path './target/*' -not -path './.git/*' \
    -not -path './oracle-work/*' | sort)
[ "$SHELL_BAD" = 0 ] || { echo "T0 RED — a shell script in this tree does not parse" >&2 || true; t0_stage_failed; }
[ "$SHELL_N" -ge 80 ] || {
    echo "T0 RED — the parse sweep examined only $SHELL_N shell scripts, floor 80" >&2
    echo "         the find selector has stopped reaching them; a sweep over nothing" >&2
    echo "         passes exactly like a sweep that found nothing wrong" >&2
    t0_stage_failed; }

# **Every published figure has to still match the artefact that produced it.**
#
# This project's figures drift away from their artefacts and every instance so
# far was caught by accident: two refuted in one day, a dashboard verdict that
# compared an r=7 measurement against an r=32 target, an accuracy percentage
# that turned out to sit below the twin-masking floor, a suspect-list size
# quoted as a defect count. `ci/figures.toml` ties each number to its artefact
# and its measurement window; `ci/check-figures.py` fails when they diverge.
#
# It is here rather than in T1 because it needs no build, no vanilla extract
# and no box — it is a few milliseconds of file reading, which is inside T0's
# budget and is the same argument as `bash -n` above. A figure check that only
# ran on the merge gate would let a wrong number sit in a branch for a day.
python3 ci/check-figures.py || { echo "T0 RED — a published figure and its artefact disagree" >&2 || true; t0_stage_failed; }

# The sound-coverage figure is the one entry in that ledger whose artefact is
# **derivable in this tree in milliseconds**, so it is re-derived rather than
# trusted. `ci/sound-coverage.py --check` recounts the extract and fails if
# the count and `ci/figures.toml` disagree.
#
# It exists because that number moved 208 -> 1,509 under a one-line change to
# how a single column is decided, and because the direction it can move
# silently is the bad one: a regenerated `use_without_item.json` that stopped
# carrying `soundEvent` would take `world_send::use_sound`'s table arm to zero
# blocks, and every check downstream would still be green — the corpus drops
# `sound.pitch` and only one scenario observes a use-path sound at all.
#
# **Skipped, not failed, when the table is absent.** `data/vanilla` is
# gitignored and generated per tree, so a fresh checkout legitimately has no
# table; exit 2 is that case and is not a branch defect.
if [ -f data/vanilla/use_without_item.json ]; then
    python3 ci/sound-coverage.py --check \
        || { echo "T0 RED — sound coverage and ci/figures.toml disagree" >&2 || true; t0_stage_failed; }
fi

# **And the five rules that adjudicate `quotable` itself have to be able to go
# red, which no live entry can prove any more.**
#
# Rules 9-13 were written on 2026-08-13 against a real fault:
# `readme-restates-ow-r16-admissible` was `quotable = true` for three days
# while the entry whose number it republishes to the block was withdrawn for
# an uncontrolled chunk order. Fixing it removed the only entry that violated
# them, so the rules now pass on an empty set — the exact "check that cannot
# fail" shape `ci/check-figures.py`'s own header says has been found
# twenty-one times here. `[meta] min_restatements` keeps the relation alive;
# this proves the rules still bite, by injecting each fault into a hermetic
# fixture and asserting the rule fires by name.
#
# It carries two GREEN controls as well as eleven red ones, and they are the
# reason it is not just a smoke test: an entry that merely *compares* against
# a withdrawn figure, and one that repeats its target's claim sentence with a
# number of its own, must both stay quotable. A rule that reds on those would
# be indistinguishable from a correct one on this ledger and wrong on the next.
#
# No build, no jar, no box — same argument as the figures check above.
# --- the unit and the question ------------------------------------------------
#
# `ci/check-figures.py` above asks whether a number and its artefact agree.
# This asks the question that class of check could not: whether the number and
# the SENTENCE IT IS PUBLISHED IN agree. Six of the run of 2026-08-18/19's
# corrections were the same shape, a right number answering a question nobody
# asked, and every one was arithmetically correct with no field in the ledger
# disagreeing with any other.
#
# It does not catch everything and its own header says so in full. A defect in
# the DEFINITION rather than the label is invisible to any string comparison —
# `frozen` counting checks with no history had an accurate label — and the rule
# for that class is stated in docs/ORACLE.md instead, because it is not
# mechanical.
python3 ci/figure-units.py \
    || { echo "T0 RED — a published number disagrees with the noun beside it" >&2; t0_stage_failed; }

python3 ci/figure-units-falsify.py > /tmp/t0-units-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/figure-units-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-units-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-units-falsify.$$.log
rm -f /tmp/t0-units-falsify.$$.log

python3 ci/check-figures-falsify.py > /tmp/t0-figures-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-figures-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-figures-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-figures-falsify.$$.log || true
tail -1 /tmp/t0-figures-falsify.$$.log
rm -f /tmp/t0-figures-falsify.$$.log

# **And the same rules in the mode THIS SCRIPT is actually running in.**
# `ci/check-figures-falsify.py`'s fixture `git init`s, so all three of its
# rule-18 cases exercise the resolving arm — and every gate directory on the box
# has no `.git`, because `ci/t0-remote.sh` ships it that way. Rule 18 therefore
# passed here by not being able to check: measured on main `1a31e0a56`, the
# identical tree read with history exits 1 with three `PROVENANCE-NO-COMMIT`
# reds and rsynced without `.git` prints `FIGURES GREEN`. Every case in this
# suite runs in a fixture with no repository, which is where this line runs.
python3 ci/figure-provenance-falsify.py > /tmp/t0-figprov-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/figure-provenance-falsify.py: rule 18 cannot go red in a gate directory" >&2
         cat /tmp/t0-figprov-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-figprov-falsify.$$.log
rm -f /tmp/t0-figprov-falsify.$$.log

# **Figures this tree publishes and did not measure.** A finished lane leaves
# its numbers on its own branch and the ledger is on main; between those two
# moments the number is either missing from the ledger or hand-copied into it,
# and a hand-copied number is indistinguishable from a measured one while
# ageing differently. `ci/checks/cited-audits.toml` registers the figure now,
# The command-tree ratchet's three refusals, injected against synthetic
# recordings. Here rather than in t2 because it needs no jar, no build and no
# box, and because the thing it defends is invisible from t2's output: the gate
# prints the same line and exits 0 whether or not its refusals still work. A
# ratchet that has stopped ratcheting looks exactly like one that is holding.
python3 ci/command-tree-parity-falsify.py > /tmp/t0-cmdtree-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/command-tree-parity-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-cmdtree-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-cmdtree-falsify.$$.log
rm -f /tmp/t0-cmdtree-falsify.$$.log

# pinned to the SHA it was measured at, and this re-reads that blob. No branch
# is merged and no measurement is repeated. Pinned to a sha and not a branch
# name deliberately: a tip moves, and a citation that follows the tip cites
# whatever that lane does next.
python3 ci/check-cited-audits.py \
    || { echo "T0 RED — a cited audit figure no longer holds on its branch" >&2 || true; t0_stage_failed; }
python3 ci/check-cited-audits-falsify.py > /tmp/t0-cited-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-cited-audits-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-cited-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-cited-falsify.$$.log || true
tail -1 /tmp/t0-cited-falsify.$$.log
rm -f /tmp/t0-cited-falsify.$$.log

# **The provenance recovery, re-derived rather than trusted.** 59 of the ledger's
# entries had their tree recovered from the tree on 2026-08-18 rather than being
# recorded when the measurement was taken, and a recovery that is not re-run is
# a hand-copied number wearing a derivation's clothes. This re-derives every
# entry `ci/figure-provenance.py` wrote and fails if one has moved — which
# happens when a claim string is edited or a document is restructured, both of
# which are ordinary and neither of which should silently change what a figure
# says it was measured on. ~15 s, no build, no jar, no lock.
#
# It refuses an EMPTY set rather than reporting green over one: wiring `--check`
# to the same selector as the default listing (entries lacking a sha) made it
# vacuous the moment the last provenance was filled in, which is the defect the
# tool was written to close, in the tool itself.
# THE PREDICATE FIRST, THE LEDGER SECOND. 2026-08-18.
#
# `--check` reported 56 of 56 figures bad on `origin/main` with nothing in the
# ledger edited, because `git log --format=%h` abbreviates to whatever keeps a
# name unique in THIS object store and that length grew from 8 to 9. The
# comparison was of spellings, so every entry stopped matching at once — and
# nothing could have said so, because the only symptom was a red gate on a
# ledger nobody had touched.
#
# So the comparison is driven before it is used. Nine cases, both abbreviation
# directions and both arms of the predicate; if it can no longer refuse, or can
# no longer accept, the gate stops here rather than reporting a verdict about
# figures that has stopped being about figures.
python3 ci/figure-provenance.py --selftest > /tmp/t0-prov-selftest.$$.log 2>&1 \
    || { echo "T0 RED — ci/figure-provenance.py self-test: the provenance comparison no longer compares" >&2
         cat /tmp/t0-prov-selftest.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-prov-selftest.$$.log || true
tail -1 /tmp/t0-prov-selftest.$$.log
rm -f /tmp/t0-prov-selftest.$$.log

python3 ci/figure-provenance.py --check > /tmp/t0-provenance.$$.log 2>&1 \
    || { echo "T0 RED — a figure's recovered provenance no longer derives" >&2
         grep -E "^MISMATCH " /tmp/t0-provenance.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-provenance.$$.log || true
tail -1 /tmp/t0-provenance.$$.log
rm -f /tmp/t0-provenance.$$.log

# The draw-log truncation guard in `ci/decorate-once.sh`, shown refusing before
# it is shown passing. Six cases, two of them refusals, and it exits 2 if that
# script has stopped carrying the guard at all — so deleting the guard is a red
# rather than a suite quietly testing a copy of nothing. It runs no JVM and
# takes milliseconds. `docs/DETERMINISM.md` 52.
./ci/decorate-once-falsify.sh > /tmp/t0-decorate-once-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/decorate-once-falsify.sh: the truncation guard stopped discriminating" >&2
         cat /tmp/t0-decorate-once-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-decorate-once-falsify.$$.log
rm -f /tmp/t0-decorate-once-falsify.$$.log

# **The append-only collision class, made structurally impossible rather than
# mechanically resolvable.** `docs/DETERMINISM.md` and `docs/PARITY-ROADMAP.md`
# are append-only with hand-numbered `## N.` sections; two branches that each
# claim a number at the same anchor used to collide textually and often on the
# identical number, costing an orchestrator turn per collision (merge chains
# 32-34 stranded nine branch-slots on exactly this). `docs/claims/*.toml` is
# the registry — one file per claimed `(doc, number)`, so two branches claiming
# at once produce two files, not one conflicted line — and
# `ci/check-doc-claims.py` is the adjudication: refuses on a duplicate
# `(doc, number)`, naming both claimants, and on either side drifting from the
# heading it registers. Needs no build and no box, so it belongs beside the
# figures check above.
python3 ci/check-doc-claims.py \
    || { echo "T0 RED — a doc claim collided, or drifted from its heading" >&2 || true; t0_stage_failed; }

# --- every DEFAULTED `Blocks` method is delegated by the production world ---
#
# `crate::edits::Blocks` is the world every ported mechanic reads through.
# Most of its methods are required and a new one breaks every impl until it
# is written, which is a compiler error and needs no gate. The handful with
# **default bodies** are a different kind of thing: the default exists so the
# many test fakes in this crate do not each have to invent an answer, it
# returns `None`, and every caller reads `None` as "the table cannot answer,
# do nothing".
#
# So a defaulted method that `crate::tiles::Tiles` — the production world —
# does not delegate compiles, answers `None`, and makes its mechanic **do
# nothing**, which is exactly what a correct server looks like when the
# mechanic simply has not fired. Every unit test goes on passing, because the
# fakes those tests use override the very method the real world drops.
#
# Two shipped that way and T0 was green on both: `level_min_y`, without which
# `FallingBlock.tick`'s whole arm is skipped and sand does not fall; and
# `note_block_instrument`, without which a note block is placed carrying the
# golden harness's instrument instead of the ground's. Neither was found by a
# gate — both by reading `Tiles`.
#
# Needs no build and no box, so it belongs here beside the checks above.
# `ci/check-blocks-delegation.sh --falsify` is its red-when-broken evidence:
# it removes each delegation in turn and requires a refusal.
./ci/check-blocks-delegation.sh \
    || { echo "T0 RED — a defaulted Blocks method is not delegated by Tiles, so it silently answers None on the real server" >&2 || true; t0_stage_failed; }
./ci/check-blocks-delegation.sh --falsify > /dev/null \
    || { echo "T0 RED — check-blocks-delegation did not refuse a removed delegation" >&2 || true; t0_stage_failed; }

# --- and the two halves of a claim that check-doc-claims cannot see ---------
#
# `ci/check-doc-claims.py` adjudicates the registry against the headings. It
# says nothing about whether the EVIDENCE inside a section is still true, and
# 2026-08-18 found two ways that rots silently, both in rows stamped CLOSED.
#
# A source citation. The neighbour-update row is CLOSED and cites
# `power.rs::update_neighbors_at` (`:856`); the symbol is at `:881` and `:856`
# is a comment about thread locals. Nineteen citations in that document were
# stale, one by 670 lines, and a reader following one to check the CLOSED
# stamp finds unrelated code with no way to tell whether the claim moved or
# the source did. `ci/check-source-citations.py` needs no ledger because the
# citation states its own oracle — the symbol and the line are both written
# down, so the check is whether they agree.
#
# A prose count. `720`'s `about` said "thirty-four checks drive
# `container_click`" against 39 steps in 15 scenarios, and was never right in
# the units it was written in. `ci/scenario-census.py` derives the count from
# the corpus and reddens when the sentence and the tree disagree.
#
# Both need no build and no box, so they belong here beside the registry.
python3 ci/check-source-citations.py \
    || { echo "T0 RED — a source citation does not land on the symbol it names" >&2; t0_stage_failed; }

python3 ci/scenario-census.py --check \
    || { echo "T0 RED — a quoted corpus figure disagrees with the corpus" >&2; t0_stage_failed; }

# --- and the third form of evidence: a commit sha ---------------------------
#
# A sha is the strongest citation here — the one that cannot be reworded — and
# the easiest to lose, because it rots into NO answer rather than a wrong one,
# and only for other people. Branches get rebased and stranded branches get
# deleted; the commit becomes an unreachable loose object, still resolvable in
# the clone where it was written and `bad object` in every other.
#
# Measured 2026-08-18 across PARITY-ROADMAP, DETERMINISM and WORKLIST: 106
# cited shas, 79 on main, 4 on a branch, 1 a blob cited as a blob, 2 belonging
# to another repository, and **20 that no ref reaches**.
#
# Those 20 cannot be repaired from here and MAX_UNREACHABLE is therefore a
# ratchet, not an absolute — guessing a replacement sha turns a citation
# nobody can resolve into one that resolves to the wrong thing. What is
# gateable is that the number never grows. 5 s, no build, no box.
python3 ci/check-cited-commits.py \
    || { echo "T0 RED — a cited commit sha resolves to nothing, or the unreachable ratchet grew" >&2; t0_stage_failed; }

# The instruments' own selftests, for the reason `ci/check-figures-falsify.py`
# runs above: a checker whose rules have silently stopped being able to fire
# is indistinguishable from a clean tree, and both of these are ledger-shaped
# enough to fail that way. `ci/claim-number.py --selftest` is here too because
# the allocator's compare-and-swap is the thing that keeps two lanes off one
# number, and a CAS that has stopped refusing looks exactly like no collisions.
for probe in "ci/check-source-citations.py --selftest" \
             "ci/scenario-census.py --selftest" \
             "ci/check-cited-commits.py --selftest" \
             "ci/claim-number.py --selftest"; do
    # shellcheck disable=SC2086
    python3 $probe > /tmp/t0-claims-probe.$$.log 2>&1 \
        || { echo "T0 RED — $probe failed:" >&2; cat /tmp/t0-claims-probe.$$.log >&2
             rm -f /tmp/t0-claims-probe.$$.log; t0_stage_failed; }
done
rm -f /tmp/t0-claims-probe.$$.log

# `ci/renumber-claim.sh` performs SIDE EFFECTS on a branch, and the failure it
# has to be guarded against is a file that should not have changed changing.
# That cannot be asserted about a reimplementation, so its falsifier builds a
# throwaway repository and runs the real script against it.
#
# Two of its cases are here because they already happened. The script had no
# `<doc>` argument and rewrote `§N` across BOTH registered documents, surviving
# three real renumbers only because no sibling happened to carry the contended
# number — case 3 makes "the sibling is byte-identical" a property rather than
# luck. And this gate caught the script citing `PARITY-ROADMAP §45`, a section
# it had itself just renumbered — case 4 makes that permanent, distinguishing
# the orphan being DETECTED from the ratchet REFUSING, which are different
# claims and were conflated the first time the case was written.
ci/renumber-claim-falsify.sh > /tmp/t0-renumber-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/renumber-claim-falsify.sh: a renumber moved something it must not, or missed something it must" >&2
         cat /tmp/t0-renumber-falsify.$$.log >&2; rm -f /tmp/t0-renumber-falsify.$$.log; t0_stage_failed; }
rm -f /tmp/t0-renumber-falsify.$$.log

# --- refuse any tracked file still carrying a conflict marker ---------------
#
# 2026-08-16: a branch reached the gate with `docs/DETERMINISM.md`
# concatenated five times — 32,577 lines against main's 6,762 — carrying
# stray diff3 conflict markers from a botched rebase resolution. It burned two
# gate attempts before anyone looked. Nothing in T0 refused it:
# `ci/merge-appendonly.py` behaved correctly (a real conflict gets markers and
# a non-zero exit, proven by `ci/merge-appendonly-falsify.sh` below), but the
# agent committed the marked-up file anyway, and the gate only noticed by
# accident — `ci/check-figures.py` happened to find every figure appearing
# exactly 5x the ledger's count. A file with no registered figures would have
# merged. That is a symptom catching a corruption, not a control.
#
# 2026-08-17, and this is a correction to the paragraph above rather than an
# addition to it: "`ci/merge-appendonly.py` behaved correctly" was true of the
# driver against its own documented contract and false about the outcome. The
# conflict it handed that agent was the WHOLE FILE between markers — every
# line of both sides, 51,298 lines in one hunk on the worst branch measured
# today — because the driver refused outright whenever either side had touched
# existing text. Nobody resolves a hunk that size by reading it, so the
# concatenation this check exists to catch is what a human does next. The
# driver now falls back to git's own line-level 3-way and refuses only the
# hunks where both sides changed the same text; five of the six worldgen
# branches blocked that morning had no such hunk at all. This check stays
# exactly as it is — the fix removes the pressure, not the failure mode.
#
# `ci/check-conflict-markers.py` is the control: it reads every marker form
# git can leave — `<<<<<<<`, the diff3 `|||||||` middle marker that is the one
# a `<<<<<<<`/`>>>>>>>`-only guard would have missed here, `=======`, and
# `>>>>>>>` — anchored at true column 0 so a line of prose that happens to
# contain seven angle brackets is not a false positive, and names the file
# and the line it refuses on. It walks the filesystem rather than asking git
# `ls-files`, for the same reason `ci/setup-merge-drivers.sh` has a no-`.git`
# fallback: `ci/t0-remote.sh` rsyncs with `--exclude '.git'`, so T0 usually
# runs where `.git` does not exist at all, and a git-dependent guard would
# pass its author's worktree and then run against nothing in every gate this
# project actually runs. No build, no box — a few hundred milliseconds of
# file reading, same argument as the figures and doc-claims checks above.
python3 ci/check-conflict-markers.py \
    || { echo "T0 RED — a tracked file carries a conflict marker" >&2 || true; t0_stage_failed; }

# `| grep -q` under `set -o pipefail` reads a MATCH as absent whenever the
# producer is still writing when grep exits (SIGPIPE, 141). It redded the
# gate-instruments gate on an unchanged file on 2026-09-24; every ci/ site
# (127 lines in 34 files on grepq-pipefail, 68 more that landed after it,
# found on train-c) was rewritten to `| grep -c PAT >/dev/null`. The
# selftest refuses each form of the pattern, accepts each repair, and shows the
# lie live on this box before the scan is trusted. File reading only.
python3 ci/check-shell-footguns.py --selftest \
    || { echo "T0 RED — ci/check-shell-footguns.py --selftest: a footgun rule stopped biting" >&2 || true; t0_stage_failed; }
python3 ci/check-shell-footguns.py \
    || { echo "T0 RED — a ci/ shell file pipes into grep -q (a match can read as absent under pipefail)" >&2 || true; t0_stage_failed; }

# Its own falsification, on the pattern of ci/merge-appendonly-falsify.sh:
# injects each of the four marker forms into a throwaway tracked file and
# proves the guard refuses every one by name and line, then proves it stays
# green on a clean tree and on the real files that legitimately contain
# marker text as string literals or grep patterns
# (`ci/merge-appendonly.py`, `ci/merge-appendonly-falsify.sh`) — see that
# script's own header for why no path allowlist was needed for either.
./ci/check-conflict-markers-falsify.sh > /tmp/t0-conflict-markers-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-conflict-markers-falsify.sh: an injected marker went unrefused" >&2
         cat /tmp/t0-conflict-markers-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-conflict-markers-falsify.$$.log || true
tail -1 /tmp/t0-conflict-markers-falsify.$$.log
rm -f /tmp/t0-conflict-markers-falsify.$$.log

# **A test that skips is a test that passed, and no log grep can see it.**
# `ci/t1-gate.sh` refuses a run whose tests skipped by grepping its log for
# `skipping: no vanilla`. That grep has returned 0 in all 143 recorded runs and
# always will, for two independent reasons: `cargo test` never prints a passing
# test's stdout or stderr, and a test that returns early passes; and the
# pattern matches 27 of the 119 skip wordings in this workspace, missing the
# largest one (`skipping: no datagen extract`, 36 sites) entirely.
#
# Measured 2026-08-18 in ~/rustmc-t1 on `main`, with `oracle-work/vanilla-26.2`
# moved aside: 111 test binaries, 1749 passed against 1758 with it present, 8
# ignored either way, `grep -c skipping` **0**, and the suite finished in 122 s
# instead of 1521 s. Every floor in `t1-known-red.toml` passed. See
# `docs/PARITY-ROADMAP.md` §73.
#
# So the count is gated here, in the source, where it needs nothing to have
# run. `ci/checks/fixture-skips.toml` is a ratchet in the shape of
# `t1-known-red.toml`: above the declaration is a new hole, below it is a stale
# declaration, and both are red. It is in T0 rather than T1 because it is a
# file read of a few hundred milliseconds and because a hole of this class
# should not wait eleven minutes to be reported.
# **Exit 2 is a refusal, not a red, and the word carries it.** Both checks here
# return 1 for "the tree disagrees with its declaration" and 2 for "I could not
# read my own inputs" — a missing or unparseable TOML, which says nothing about
# the branch. Collapsing those into one message is the same fault as a stage
# reporting RED for an environment fault: it sends the wrong lane looking. The
# exit stays 1 either way, exactly as the `T0 REFUSED` for a half-generated
# `data/vanilla` above does, because `ci/handoff.sh` greps for the word.
# `cmd; VAR=$?` IS A DEAD HANDLER UNDER `set -e` — 2026-09-06.
# This file is `set -euo pipefail`, and `cmd; VAR=$?` is a sequential list,
# so errexit is not suspended: the shell exited AT the python line with its
# status and the whole `case` below — including the REFUSED arm this comment
# block argues for — was unreachable. T0 then exited 1 or 2 having printed
# neither `T0 REFUSED` nor `T0 RED`, and `ci/handoff.sh` greps for the word.
#   $ bash -c 'set -euo pipefail; false; RC=$?; echo reached'; echo "exit=$?"
#   exit=1        # nothing printed
# Same defect, same repair as `ci/order-selfconsistency.py` on
# `origin/gate-instruments` 5adcd3f6c.
SKIPS_RC=0
python3 ci/check-fixture-skips.py || SKIPS_RC=$?
case $SKIPS_RC in
    0) ;;
    2) echo "T0 REFUSED — ci/check-fixture-skips.py could not read its declaration" >&2
       echo "             nothing about the branch was measured." >&2
       t0_stage_failed ;;
    *) echo "T0 RED — a fixture skip was added, or the ratchet was not lowered" >&2
       t0_stage_failed ;;
esac

# **A generator nothing asks for is a table nobody makes.** The forward check
# — every derived table has a `generator_for()` arm — lives in
# `ci/gen-gameplay-tables.sh` and has since the start. The reverse one did
# not, and the reverse one is the silent half: `block-entities-2` added a
# correct arm and a load site spelled `.join("../../data/vanilla/...")`, which
# the deriving grep cannot match, so the arm was reachable from nothing, the
# script said `21 tables current` and exit 0, the golden was never generated
# in any gate directory, and three tests panicked on a missing fixture — T0
# exit 101 — while the lane's own tree, holding the table by hand, was green.
# It is a T0 check and not a block inside that script because the property is
# about the real repository: `ci/gen-tables-falsify.sh` runs
# `gen-gameplay-tables.sh` against hermetic fixture trees where every arm is
# legitimately unreached, and a guard living there aborted ten of its cases.
./ci/check-generator-arms.sh || t0_stage_failed

# Its own red-when-broken evidence, absent until 2026-09-05. This guard's whole
# content is "two derived sets agree", and it can be wrong in two opposite and
# equally quiet ways: miss an arm nobody wired (which it caught on
# `gate-instruments`, `tags_block.json`), or go on exempting a
# `DORMANT_GENERATORS` entry long after something started reading that table,
# at which point the exemption stands ready to hide the next arm somebody
# forgets. Seven cases in a temp `$ROOT`, two of them controls, because the real
# repository has exactly one dormant arm and no unreached one — so every case
# would be unreachable against it.
./ci/check-generator-arms-falsify.sh > /tmp/t0-genarms-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-generator-arms-falsify.sh: an injected fault went green" >&2
         cat /tmp/t0-genarms-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-genarms-falsify.$$.log
rm -f /tmp/t0-genarms-falsify.$$.log

# Its own red, on the pattern above: an added raw skip must be refused, a
# converted site must be refused until the ratchet drops, and the unmodified
# tree must be green so neither red is an artefact of the harness.
python3 ci/check-fixture-skips.py --falsify > /tmp/t0-fixture-skips-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-fixture-skips.py --falsify: an injected skip went green" >&2
         cat /tmp/t0-fixture-skips-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-fixture-skips-falsify.$$.log
rm -f /tmp/t0-fixture-skips-falsify.$$.log

# **What has ever been shown to make each T1 binary fail.** §73.7 counted 74 of
# 80 with no positive control at all; §84 gives each of them one or names it. The
# three checks here are the ones a merge gate can afford on every run — the
# arithmetic, the sabotage feature being off, and no fixture left held aside.
# Re-running the 61 controls is `ci/t1-positive-control.py --emit`, minutes not
# milliseconds.
#
# The `2`/`*` split is the same `T0 REFUSED`/`T0 RED` distinction as the block
# above: an unreadable ledger or an absent `cargo` says nothing about the branch.
T1C_RC=0; python3 ci/check-t1-controls.py || T1C_RC=$?
case $T1C_RC in
    0) ;;
    2) echo "T0 REFUSED — ci/check-t1-controls.py could not read its inputs" >&2
       echo "             nothing about the branch was measured." >&2
       t0_stage_failed ;;
    *) echo "T0 RED — the T1 positive-control ledger no longer describes this tree" >&2
       t0_stage_failed ;;
esac

# **A golden nobody opens is a gate that is not running.** Ten of the fifty
# fixtures under a `golden/` directory are named by no Rust source: six
# exported from Minecom for functions with no port in this workspace, two
# savanna village captures `golden_village_start` never reads, one aquifer
# centre dump no assertion consumed, and one — `decorate_vectors_iceberg.json`
# — that really is read, through `RUSTMC_DECORATE_GOLDEN` in `ci/t1.sh`, which
# is why the reader corpus is Rust and not `ci/*.sh`: counting shell would also
# have counted `ci/village-pieces.sh`, the *producer* of two of the dead ones.
#
# Exact in both directions, like the ratchet above: a new orphan is red, and a
# declared orphan that gains a reader is red as a stale declaration.
# `cmd; VAR=$?` under `set -e`: see the block at `SKIPS_RC` above.
READERS_RC=0
python3 ci/check-golden-readers.py || READERS_RC=$?
case $READERS_RC in
    0) ;;
    2) echo "T0 REFUSED — ci/check-golden-readers.py could not read its declaration" >&2
       echo "             nothing about the branch was measured." >&2
       t0_stage_failed ;;
    *) echo "T0 RED — a golden fixture lost its reader, or an orphan declaration is stale" >&2
       t0_stage_failed ;;
esac

python3 ci/check-golden-readers.py --falsify > /tmp/t0-golden-readers-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-golden-readers.py --falsify: an orphaned golden went green" >&2
         cat /tmp/t0-golden-readers-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-golden-readers-falsify.$$.log
rm -f /tmp/t0-golden-readers-falsify.$$.log

# ===== THE CENSUS OF CHECKS WHOSE GREEN CANNOT MEAN ANYTHING — 2026-09-06 =====
#
# `docs/AGENT-BRIEF.md`'s HARD RULE names this project's central defect: an
# instrument that measures its own input, or a verdict whose green cannot be
# turned red. It has always been found one instance at a time, by a lane
# looking for something else — five of them in one night on 2026-09-05, none by
# anybody hunting for them. `ci/vacuous-green-census.py` derives the whole
# population from the source; this refuses a NEW member of it, and equally
# refuses a declaration left standing after its instance was repaired.
#
# Three exit codes, three meanings, and the `|| RC=$?` form because `cmd; RC=$?`
# under this file's own `set -euo pipefail` is the very shape being gated (see
# the block at `SKIPS_RC` above).
VACGREEN_RC=0
python3 ci/check-vacuous-green.py > /tmp/t0-vacuous-green.$$.log 2>&1 || VACGREEN_RC=$?
case $VACGREEN_RC in
    0) ;;
    2) echo "T0 REFUSED — ci/check-vacuous-green.py could not derive its census" >&2
       cat /tmp/t0-vacuous-green.$$.log >&2
       t0_stage_failed ;;
    *) echo "T0 RED — a check that cannot go red was added, or a declaration is stale" >&2
       cat /tmp/t0-vacuous-green.$$.log >&2
       t0_stage_failed ;;
esac
head -2 /tmp/t0-vacuous-green.$$.log
rm -f /tmp/t0-vacuous-green.$$.log

# Its own red-when-broken evidence. A ratchet on a census is worth exactly what
# the census is worth, and a census that returns nothing passes every
# comparison built on it — which is how one falsifier passed green on
# 2026-09-05 by mutating an empty dict.
./ci/check-vacuous-green-falsify.sh > /tmp/t0-vacgreen-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-vacuous-green-falsify.sh: the vacuous-green guard did not refuse an injected instance" >&2
         cat /tmp/t0-vacgreen-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-vacgreen-falsify.$$.log
rm -f /tmp/t0-vacgreen-falsify.$$.log

# `ci/t2-gameplay.sh` had no string for its own failure until 2026-08-18: GREEN
# on success, REFUSED when it declined to measure, and on a corpus regression it
# exited non-zero saying neither. The gate was still RED (handoff reads the exit
# status) but every reader of the LOG was blind — `ci/gate-rate.py` scored T2 at
# 143 GREEN / 0 RED across the project's whole history, and the waiter
# `docs/AGENT-BRIEF.md` recommends hung forever through exactly that case.
#
# The sentinel is additive, so the thing worth gating is not that it appears but
# that it appears ONLY on a failing exit: one that fired on a green run would
# turn every gate red. This falsifier asserts both, exercises the REAL script
# through two injections, and takes no corpus lock (both injections kill it
# before `corpus_lock_acquire`), so it belongs in T0 rather than beside a gate.
./ci/t2-verdict-falsify.sh > /tmp/t0-t2-verdict-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/t2-verdict-falsify.sh: ci/t2-gameplay.sh's failure is silent, or its sentinel fires when it should not" >&2
         cat /tmp/t0-t2-verdict-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-t2-verdict-falsify.$$.log || true
tail -1 /tmp/t0-t2-verdict-falsify.$$.log
rm -f /tmp/t0-t2-verdict-falsify.$$.log

# --- the three scripts every lane was copying in by hand --------------------
#
# `ci/run-detached.sh`, `ci/unlanded.sh` and `ci/claim-number.py` all existed,
# all had falsifiers, and none of them was on `main`. They were also all three
# in `.git/info/exclude`, which is shared by every worktree of this repository,
# so a lane that copied one in and ran `git add` was told its own script was
# ignored. They are tracked now; these are the gates that keep them working.

# THE BLESSED DETACHED-WAIT HARNESS. `docs/AGENT-BRIEF.md` tells every agent to
# run long sequences through `ci/run-detached.sh` and arm one waiter on the
# sentinel it writes. A sentinel absent on a failure hangs that waiter forever,
# which is exactly what `ci/t2-gameplay.sh`'s missing RED line did to the
# waiter recommended two blocks above this one — and a hung waiter is
# indistinguishable from a long job, so it is silent for as long as anyone is
# willing to wait. The falsifier drives the REAL script through the outcomes a
# payload has no string for: a non-zero exit it never names, total silence, an
# unparseable body, a payload killed -9 (rc=137), and a duplicate name that
# would make two runs share one sentinel. ~8 s, no box, no corpus lock.
./ci/run-detached-falsify.sh > /tmp/t0-run-detached-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/run-detached-falsify.sh: the detached-wait sentinel can be absent on a failure" >&2
         cat /tmp/t0-run-detached-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-run-detached-falsify.$$.log
rm -f /tmp/t0-run-detached-falsify.$$.log

# HOW BIG THE BACKLOG IS. `docs/AGENT-BRIEF.md` tells lanes to size a branch
# family with `git cherry` and never `git rev-list --count`, because this repo
# lands by rebase and a rebased copy of a landed patch carries a new sha. That
# is a claim about git's behaviour written in prose, which is a claim nobody
# re-checks, and it has already misled a lane and the orchestrator. The
# falsifier builds a scratch repo containing exactly one already-landed commit,
# one stack and one duplicated patch, so the four numbers are 8/5/4/3 by
# construction, and asserts `ci/unlanded.sh` prints them rather than asserting
# git's behaviour alone — a correct explanation shipping beside a wrong
# implementation is the failure this project keeps paying for. ~2 s, no remote.
./ci/unlanded-falsify.sh > /tmp/t0-unlanded-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/unlanded-falsify.sh: the backlog instrument miscounts, or git cherry no longer filters landed patches" >&2
         cat /tmp/t0-unlanded-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-unlanded-falsify.$$.log
rm -f /tmp/t0-unlanded-falsify.$$.log

# THE SECTION-NUMBER ALLOCATOR'S COMPARE-AND-SWAP. `ci/check-doc-claims.py`
# above is the detector and it fires at merge, on a tree that already contains
# both claims — by which point the collision has cost both lanes their
# numbering and a third party the renumber. `ci/claim-number.py` is the
# allocator that stops it happening, and the only thing keeping two lanes off
# one number is that its CAS can LOSE. A CAS that has silently stopped
# refusing looks exactly like a day with no collisions, which is this project's
# central defect class. `--selftest` poisons each refusal on fixtures and
# asserts it fires. Milliseconds, no ref-store write.
python3 ci/claim-number.py --selftest > /tmp/t0-claim-number.$$.log 2>&1 \
    || { echo "T0 RED — ci/claim-number.py --selftest: the allocator's compare-and-swap has stopped refusing" >&2
         cat /tmp/t0-claim-number.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-claim-number.$$.log
rm -f /tmp/t0-claim-number.$$.log

# --- the command-tree extract's own adjudicator ----------------------------
#
# `ci/command-tree-extract.sh` runs the dumper twice and no longer `cmp`s: on
# 2026-09-05 sixteen runs of the same class against the same jar produced two
# documents, differing in 160 of 2,085 nodes, every one a child of /gamerule.
# So it forgives a permutation under a named root and refuses everything else,
# and an allowance like that is exactly the shape that stops being able to
# fail. `--selftest` builds synthetic pairs and asserts each refusal fires,
# including the two a naive "ignore gamerule" allowance would wave through: a
# child whose level moved and a child whose executability moved.
# Milliseconds, no JVM.
python3 ci/command-tree-compare.py --selftest > /tmp/t0-cmdtree-cmp.$$.log 2>&1 \
    || { echo "T0 RED — ci/command-tree-compare.py --selftest: the extract's two-run adjudicator has stopped refusing" >&2
         cat /tmp/t0-cmdtree-cmp.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-cmdtree-cmp.$$.log
rm -f /tmp/t0-cmdtree-cmp.$$.log

# --- and the two things deliberately NOT gated here, with the reason --------
#
# `ci/claim-number.py --check` is NOT run here, though `origin/claims-allocator`
# ran it. It reports whether THIS tree's claims collide with any other ref, and
# on main at e2fc07600 the honest answer is that two do: DETERMINISM §46 and
# PARITY-ROADMAP §57, each held by main and by three unmerged branches under a
# different slug. Gating it would therefore red T0 for every lane on the box
# today, over a collision no lane can repair from its own tree — the branches
# have to renumber. A ratchet on the count is no better: the count is a
# function of ALL refs, so any other lane pushing a branch could red your gate,
# and a gate that fires on the innocent gets ignored, then removed. The
# script's own docstring says it plainly — "`--check` is a pre-flight a lane
# runs on itself; the gate is still `ci/check-doc-claims.py`" — and that branch
# put into T0 the thing its author had documented as not a gate. Run it
# yourself before you write a `## N.` heading.
#
# `ci/claim-number-falsify.sh` (23 cases) is not run here either, for a
# mechanical reason rather than a philosophical one: two of its cases poison
# real files under `docs/claims/` and restore them on an EXIT trap. A gate
# killed with -9 mid-case would leave the poison behind, and a dirty worktree
# makes `ci/handoff.sh` REFUSE — so gating it trades a rare instrument
# regression for a stuck queue. Run it after touching either script.

# --- refuse a tree that has stopped running one of its own checks -----------
#
# The guard above can only refuse a marker in a tree that still runs it. On
# 2026-08-17 a branch was found whose tree had no `ci/check-conflict-markers.py`
# at all — rebased onto a main that predated it — so the check did not fail, it
# did not happen, and queue triage recorded that as GREEN. Three queued
# branches were in that state; none of them was actually carrying a marker, so
# it cost nothing that day, and it was found by accident both times it was
# found.
#
# Two of the three doors are shut elsewhere and neither is this file:
# `ci/handoff.sh` refuses any branch BEHIND main (exit 3), so a stale `ci/`
# cannot reach the gate at all, and `preflight.sh` now reports `missing:<check>`
# instead of skipping. What is left is a branch that deletes a check AND its
# invocation here in the same commit — nothing is inconsistent then, and T0 is
# green because it has stopped looking. `ci/checks/required-checks.toml` is the
# thing such a branch would also have to delete, and the count is bounded by a
# constant in the script so that removing one is a reviewed change.
python3 ci/check-checks-present.py \
    || { echo "T0 RED — a required check is missing, or present and wired to nothing" >&2 || true; t0_stage_failed; }

./ci/check-checks-present-falsify.sh > /tmp/t0-checks-present-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-checks-present-falsify.sh: a removed check went unrefused" >&2
         cat /tmp/t0-checks-present-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-checks-present-falsify.$$.log
rm -f /tmp/t0-checks-present-falsify.$$.log

# --- ci/handoff.sh's TERMINAL STATES ----------------------------------------
#
# `ci/handoff.sh` is the only route to `main` and had never been owned as an
# area. Its record is 41 GREEN / 32 RED and until 2026-08-18 the chain had never
# once named a RED — 30 of the 32 arrived as "no verdict line" and 2 as another
# branch's MERGED. These five gate the other half of that question: whether the
# gate EMITS an unambiguous verdict on every path it can take.
#
# THEY BELONG IN T0 AND NOT BESIDE A GATE. Every one of them drives the real
# `ci/handoff.sh` in a throwaway repository with a stubbed box: no ssh to the
# real host, no cargo, no corpus lock, and a private gate-lock directory, so
# none of them can queue behind or wedge a live gate. Measured cost, added to
# T0 in this order: 0.1 s, 1.5 s, 5.4 s, 9.7 s, 8.0 s — about 25 s.
#
# The order is deliberate: the declaration check is first because it is
# instantaneous and because a stale declaration makes the reports below harder
# to read, and its own falsifier is second because a check that cannot fail
# makes everything after it worthless.
python3 ci/check-handoff-terminals.py \
    || { echo "T0 RED — ci/check-handoff-terminals.py: a terminal state of ci/handoff.sh is undeclared, misnamed, or has no control" >&2 || true; t0_stage_failed; }
python3 ci/check-handoff-terminals-falsify.py > /tmp/t0-handoff-terminals-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-handoff-terminals-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-handoff-terminals-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-handoff-terminals-falsify.$$.log
rm -f /tmp/t0-handoff-terminals-falsify.$$.log

./ci/handoff-state-falsify.sh > /tmp/t0-handoff-state.$$.log 2>&1 \
    || { echo "T0 RED — ci/handoff-state-falsify.sh: a refusal before the box says the wrong word or the wrong exit code" >&2
         cat /tmp/t0-handoff-state.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-handoff-state.$$.log
rm -f /tmp/t0-handoff-state.$$.log

./ci/handoff-verdict-selftest.sh > /tmp/t0-handoff-verdict.$$.log 2>&1 \
    || { echo "T0 RED — ci/handoff-verdict-selftest.sh: a stage failure reaches the wrong verdict" >&2
         cat /tmp/t0-handoff-verdict.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-handoff-verdict.$$.log
rm -f /tmp/t0-handoff-verdict.$$.log

# The merge tail is the half that has already changed `main` by the time it can
# fail, and the four terminals in it are the ones that leave `main` and
# `origin/main` able to disagree. One unpushed merge makes every branch behind
# it in the queue read `N behind main` and refuse, which is 2026-08-16.
./ci/handoff-merge-tail-falsify.sh > /tmp/t0-handoff-tail.$$.log 2>&1 \
    || { echo "T0 RED — ci/handoff-merge-tail-falsify.sh: a merge, push or partial-failure path is unnamable, or it wedges the queue" >&2
         cat /tmp/t0-handoff-tail.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-handoff-tail.$$.log
rm -f /tmp/t0-handoff-tail.$$.log

# `docs/DETERMINISM.md merge=appendonly` in `.gitattributes` names a driver
# git does not know about until `ci/setup-merge-drivers.sh` registers it
# **locally** — an unregistered driver name falls back to git's default merge
# with no warning, which is the exact textual collision the driver exists to
# remove. Here it can only run its no-`.git` fallback (text-checks
# `.gitattributes` and that the driver parses): T0 reaches this line by way
# of `ci/t0-remote.sh --run`, which rsyncs with `--exclude '.git'` on
# purpose — see that script's header. The `git config`/`git check-attr`
# registration that actually matters happens once per sync, against the
# real local worktree, from `ci/t0-remote.sh` itself, before anything ships.
#
# SINCE 2026-08-16 IT IS TWO DRIVERS, AND EVERYTHING ABOVE APPLIES TO BOTH.
# `.gitattributes` also names `docregions` on `docs/WORKLIST.md`,
# `docs/PARITY-ROADMAP.md` and `ci/figures.toml`, and this stage additionally
# runs `ci/merge-docregions.py --check` over those three — which needs no git,
# so unlike `git check-attr` it still has teeth in an rsynced build copy. A
# file carrying the attribute but no longer declaring the `MERGE-REGION` it
# promises is a T0 red: the attribute would otherwise keep promising a
# behaviour the file no longer declares, and the driver's honest answer to that
# is to resolve nothing, silently. Measured that day, with the drivers inert,
# 26 of 48 unmerged branches would not rebase onto main and 18 of the 26
# conflicted on nothing but those four files.
./ci/setup-merge-drivers.sh \
    || { echo "T0 RED — a merge driver's tree-local checks failed (registration, .gitattributes, or a lost MERGE-REGION marker)" >&2 || true; t0_stage_failed; }

# Proves BOTH drivers on actual three-way merges in throwaway git repos — a
# driver that has never been exercised on a real merge is a hope, not a
# control. The suite derives and prints its own case and assertion counts — it
# said 29 and 89 for three weeks while it grew, so do not quote a constant here.
# A third of the cases are REFUSALS and they are the point: a rewritten row, a rewritten `## NOW` line, an add/add in
# `## NOW` where no region is declared, a delete-vs-edit, a duplicated table
# key, a duplicated `## N.`, a duplicated `[[figure]]` id, a duplicated
# `[[scenario]]` name, an edited `[[scenario]]` stanza beside an unrelated
# append, a rewritten baseline row, two rewrites of the SAME `ci/t0.sh` check,
# an append that breaks bash syntax, and a malformed region marker all leave
# conflict markers and a non-zero exit. That is what stops this rotting into
# the keep-both hack that produced the stale-figure defect `merge=union` once
# caused on `docs/WORKLIST.md` — whose damage is still legible there, two
# contradictory `ci-unfalsifiable-root` rows deep.
./ci/merge-appendonly-falsify.sh \
    || { echo "T0 RED — ci/merge-appendonly-falsify.sh: a merge driver is not doing what it claims" >&2 || true; t0_stage_failed; }

# `ci/admissible-residual.py` is the instrument behind the one quotable r=16
# overworld figure (`docs/DETERMINISM.md` §12). Its real inputs are two
# `region-differ --positions` runs and it cannot be exercised here, so what
# runs in T0 is its self-test: fixtures that prove it computes the
# intersection correctly AND that all four of its refusals fire. The refusals
# are the point — the one that checks the parse against the differ's own
# counter is the guard against the failure `docs/DETERMINISM.md` §10 records,
# where a loose filter let a summary line survive five intersections.
# Both streams are swallowed on the happy path: the self-test deliberately
# provokes four refusals and they print to stderr, so a green run that showed
# them would read as four failures.
python3 ci/admissible-residual.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/admissible-residual.py self-test" >&2
         python3 ci/admissible-residual.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# `ci/admissible-floor.py` is the same instrument run against a candidate that
# is known innocent — an unmodified vanilla run — which is what turns the
# admissible charge from a number into a number with a floor beside it
# (`docs/DETERMINISM.md` §14). It gets a self-test here for the same reason and
# on the same budget. Two of its seven cases already earned their place: the
# module-cache one caught a live bug on this script's first run, where loading
# the borrowed parser twice made two distinct `Malformed` classes and the
# `except` in `main` could not catch what `report` had raised.
python3 ci/admissible-floor.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/admissible-floor.py self-test" >&2
         python3 ci/admissible-floor.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# `ci/order-charge-sweep.py` recomputes that same charge under every one of our
# chunk-visitation orders **without generating anything**, by reconstructing the
# admissible set out of the `.pos` matrix. The whole reconstruction rests on one
# inference — a reference that does not list a position agrees with our default
# there — and the case that exercises it is the one nothing else would catch: a
# position no reference lists, which an order moves, and which must therefore
# become charged. A wrong reconstruction here would print a plausible table of
# thousands of blocks with nothing to contradict it.
python3 ci/order-charge-sweep.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/order-charge-sweep.py self-test" >&2
         python3 ci/order-charge-sweep.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# The two scripts that re-took the r=16 level and §4's ladder are the same kind
# of hazard and are gated the same way. Both compute a published number by set
# operations over a `.pos` matrix rather than by diffing, so a wrong operator
# prints a believable table instead of failing. What their self-tests pin is
# the two properties that would make every row wrong without making any row
# look wrong: that a charge is an INTERSECTION and therefore can only shrink as
# references are added, that a floor is a UNION and therefore can only grow,
# and that `p_hide(N) = 1 - 0.05**(1/N)` really is the inverse of the power
# statement it is quoted as. `ci/floor-ladder.py` additionally refuses at
# runtime unless it reproduces `ow-r16-floor-n2` (29,560) and the twin-masked
# residual (29,490), both of which were taken by diffing — but that check needs
# the matrix on the box, so it cannot be the gate here.
#
# `ci/floor-census.py` joined them on 2026-08-10 and is the sharpest example of
# the class, because its output is a LOOKUP TABLE that gets committed. The
# aggregate floor it prints is checked against the registered range at runtime,
# so a wrong set operator there shows up; but the per-block table beside it has
# no such anchor, and a future item is meant to adjudicate a residual against it
# instead of against 4,835. A `max` silently written as a `min`, or a union read
# as an intersection, would print a smaller floor for every block at once and
# nothing would look odd. So its self-test asserts the printed arithmetic on an
# ASYMMETRIC fixture — a symmetric one cannot tell those mutations apart — and
# it pins the three refusals that stop an unstamped or mixed-order ensemble
# being baked into a committed table (ORACLE rule 30).
#
# `ci/order-pair-decompose.py` joined them for the same reason one step down.
# It takes ONE `vanilla -> ours` row and reports it under two chunk orders, and
# the finding it exists to produce is of the form "charged under one order,
# EMPTY under the other" — a claim that a union-for-intersection slip would
# manufacture out of nothing and a set-difference slip would delete. Its
# fixture is asymmetric in both directions at once: the row is charged by three
# references in one arm and by two of three at a second position that must
# therefore NOT be charged, and the cross-order control's reference differs
# from the anchor at a position that is not in the row, so a control that
# tested the wrong set would report 1 of 1 where the answer is 0 of 1.
#
# `ci/floor-vs-n.py` and `ci/floor-anchor.py` joined on 2026-08-10 (§24) and
# both belong here for the same reason, sharpened. Neither prints a residual;
# they print the FLOOR a residual is adjudicated against, so an arithmetic
# mutation in either makes every future exactness claim on that window wrong in
# the reassuring direction. `floor-anchor.py` is the more dangerous of the two
# because it replaces an `n(n-1)` intersection with an `2n-3` reconstruction:
# its self-test asserts that dropping the branch which charges a candidate that
# AGREES with the anchor turns a 2 into a 1, which is exactly the mutation an
# obvious implementation ships with and which would understate every floor.
#
# THIS LIST IS EXPLICIT AND NOT A GLOB, so adding a script here is a step
# somebody has to remember. That is the `--lib` defect this file was fixed for
# in 2026-08-03: nineteen tests had never run in any gate because the selector
# did not reach them, and one of them was failing.
# `ci/parity-aggregate.py` joined on 2026-08-12 (PARITY-ROADMAP 37) and is the
# most dangerous member of this list, because it is the only one that composes
# a number for PUBLICATION out of two other instruments' outputs. Its whole
# value is its eight refusal arms — a stale projection, a moved denominator, a
# drifted ensemble, a census with no ceilings — and an arm that silently stops
# firing turns it into precisely the instrument 15 and 20 are about: one that
# keeps printing a confident figure through the change that invalidated it.
# Its self-test runs a green control at BOTH ends of the twelve cases, so an
# arm that fires on everything fails it as loudly as one that fires on nothing.
# `ci/window-resolving-power.py` and `ci/residual-census.py` are the same
# hazard again and were the same omission: both HAD a `--selftest` and neither
# was gated, so the checkers this project uses to decide whether a lump is a
# defect had refusals that nothing exercised. Added 2026-08-10. The census one
# additionally pins that y bands use FLOOR division — Python's `int()`
# truncates toward zero and the overworld starts at y=-64, so the wrong one
# mis-bands a quarter of the window while every row still looks plausible.
for s in ci/order-level-report.py ci/floor-ladder.py ci/floor-census.py \
         ci/order-pair-decompose.py ci/floor-vs-n.py ci/floor-anchor.py \
         ci/parity-aggregate.py ci/window-resolving-power.py \
         ci/residual-census.py ci/tick-section-classify.py; do
    python3 "$s" --selftest > /dev/null 2>&1 \
        || { echo "T0 RED — $s self-test" >&2
             python3 "$s" --selftest 2>&1 >&2 || true; t0_stage_failed; }
done

# The two jigsaw-stream controls are bash rather than python, and they are
# gated here for the reason the block above gives and one more: both of them
# were positive controls that PRINTED and never refused. A run whose dumps had
# both died reported "(no emit line found)", compared directories that did not
# exist and exited 0; the village half is worse, because it opens no reference,
# so a knob that never turned leaves three arms that are the same arm agreeing
# trivially at `mismatches 0`. Fixed and falsified 2026-08-18 — and a control
# that has only ever been seen passing is the thing this whole section exists
# to refuse, so the falsifications are gated rather than trusted. Each
# `--selftest` drives its conditions with crafted logs: it runs no dump, opens
# no reference and needs no built binary, so it costs milliseconds here.
for s in ci/jigsaw-stream-bediff.sh ci/jigsaw-stream-villages.sh; do
    "$s" --selftest > /dev/null 2>&1 \
        || { echo "T0 RED — $s self-test" >&2
             "$s" --selftest 2>&1 >&2; t0_stage_failed; }
done

# AND THE SELF-TEST ABOVE IS ITSELF GATED, on the same argument the corpus-floor
# block below makes about its own falsifications. A self-test protects the code
# as it stood on the afternoon somebody mutation-tested it by hand; the next
# person to add a branch has no way to check the fixture reaches it. On the day
# `ci/floor-census-falsify.sh` was written it caught 17 of 18 mutations and the
# survivor was live: deleting the `max(floor, 1)` clamp, which is what stops a
# charge of ONE block on a floor of ZERO reading as CLEAR. A second survivor
# turned up when the block-state key was mutated, because every block in the
# fixture was unstated — the exact mistake that produced 14 wrong rows out of
# 28 on this project's first per-block table. Both are fixed and both would
# have been invisible without this.
#
# It costs under two seconds, needs no build, no vanilla extract and no box.
# The summary line is printed on success for the reason the corpus-floor one is.
./ci/floor-census-falsify.sh > /tmp/t0-floor-census-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/floor-census-falsify.sh: a mutation survived" >&2
         cat /tmp/t0-floor-census-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-floor-census-falsify.$$.log || true
tail -2 /tmp/t0-floor-census-falsify.$$.log
rm -f /tmp/t0-floor-census-falsify.$$.log

# --- the block-entity emitter's tests, shown able to go red -----------------
#
# `crates/rustmc-worldgen/src/block_entity.rs` is the only thing writing block
# entities, and its whole protection is six unit tests that were green on their
# first run. Rule 7. `ci/be-emit-falsify.sh` injects ten transcription errors
# a careful reader could plausibly make and demands each be caught, plus one
# argued **unobservable** — insert order into a `BTreeMap` — as the control
# that proves the harness distinguishes "the tests noticed" from "the build
# changed at all".
#
# It found a hole in ITSELF on its first run and that is why it is worth
# gating rather than running once: the injection table was tab-separated, tab
# is IFS whitespace, so the two rows with an empty column had their fields
# shift left. Those two applied a nonsense patch, matched no `case` branch and
# printed nothing, and the summary still read `injections=10 holes=0`. Eight
# results for ten injections. A sweep that loses a case silently is a sweep
# that fails open; the separator is now `@` and an unreadable row is counted
# as a hole.
#
# 41 s, no build beyond an incremental debug rebuild of one crate, no vanilla
# extract, no box, no lock.
./ci/be-emit-falsify.sh > /tmp/t0-be-emit-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/be-emit-falsify.sh: an injected fault went green" >&2
         cat /tmp/t0-be-emit-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-be-emit-falsify.$$.log
rm -f /tmp/t0-be-emit-falsify.$$.log

# --- is this tree's data/vanilla actually this tree's? ----------------------
#
# ADDED 2026-08-12, out of a failure whose whole cost was that it surfaced
# here, disguised. A gate directory's regeneration died at the twelfth of
# nineteen generators (`cargo: command not found` — a non-interactive `ssh`
# has no `~/.cargo/bin`), and `ci/gen-gameplay-tables.sh` reported it correctly
# and exited 1. Nothing read that status. What T0 then ran against was a
# `data/vanilla` holding eleven of this tree's tables and eight of another
# tree's, every one present, non-empty and plausible — because `cp -rn` seeds
# every gate directory from a shared checkout, so an ungenerated table is not a
# missing one. **Eight golden-table tests and `check-use-probe-falsify` went
# red, and every one of them read as a defect in the branch under test.**
#
# The two lines below are what turns that into one sentence naming the real
# cause. This is a refusal and not a test failure: a tree whose tables are
# half somebody else's cannot produce a verdict about a branch, and reporting
# one would be a verdict drawn from nothing.
#
# Cheap and inert by design — it reads one file that only exists when a
# generation run did not finish, so a tree that has never generated tables (as
# most local checkouts have not) sees nothing.
if [ -e data/vanilla/.provenance/INCOMPLETE ]; then
    echo "T0 REFUSED — data/vanilla is half-generated; these tables are not all this tree's" >&2
    sed 's/^/    /' data/vanilla/.provenance/INCOMPLETE >&2
    echo "    Finish it with ./ci/gen-gameplay-tables.sh and re-run. Do NOT read the" >&2
    echo "    test results below as a statement about the branch." >&2
    t0_stage_failed
fi

# The generation driver's own falsification. It costs about a second, needs no
# jar, no cargo and no box — stub generators in a throwaway tree — and it is
# here for the reason the two blocks below are: the refusals it proves are in a
# script most trees run exactly once, at setup, where nobody is watching. In
# particular it pins the case the exit status cannot see, a generator that
# returns 0 having written nothing, which used to end with another tree's table
# stamped as this tree's own.
./ci/gen-tables-falsify.sh > /tmp/t0-gen-tables-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/gen-tables-falsify.sh: a generation failure went unreported" >&2
         cat /tmp/t0-gen-tables-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-gen-tables-falsify.$$.log || true
tail -1 /tmp/t0-gen-tables-falsify.$$.log
rm -f /tmp/t0-gen-tables-falsify.$$.log

# The same argument one layer up, on a golden TABLE rather than on a floor.
# `ci/check-use-probe.py` is what stops `data/vanilla/use_without_item.json`
# recording one player's behaviour as the block's — 17 of its 184 rows move when
# the player turns round, and until 2026-08-11 nothing could have said so.
# Gated here because that check runs inside a generator most trees never invoke,
# and a refusal nobody exercises is a refusal that has stopped working. Its
# fixture is synthetic, so this needs no jar, no table and no box; it also runs
# a green control against the real table when the tree has one.
./ci/check-use-probe-falsify.sh > /tmp/t0-use-probe-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-use-probe-falsify.sh: an injection went green" >&2
         cat /tmp/t0-use-probe-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-use-probe-falsify.$$.log || true
tail -1 /tmp/t0-use-probe-falsify.$$.log
rm -f /tmp/t0-use-probe-falsify.$$.log

# The golden generators' refusal ledger. Two things run here and they answer
# different questions.
#
# `ci/refusal-ledger-falsify.py` is hermetic — synthetic tables, no jar, no
# `data/vanilla`, no box — so it runs in every tree and pins **every** refusal
# that keeps the ledger honest. It counts its own arms and prints the number;
# this comment used to say "the nine" and there have not been nine of them for
# some time. It is here rather than beside the generators because those run
# once at setup, where nobody is watching.
#
# `--check` re-derives the refusal populations from this tree's own tables
# and refuses if any has moved. It needs the tables, so it is conditional: a
# tree that has never generated them says so and does not report a ledger of
# zeroes. A count that moved is either a jar change or a generator change, and
# either way every doc quoting it is now wrong — which is the whole reason the
# ledger exists, because the alternative was `use_without_item.json`'s header
# claiming 199 refusals against a table carrying 160.
python3 ci/refusal-ledger-falsify.py > /tmp/t0-refusal-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/refusal-ledger-falsify.py: an arm did not fire" >&2
         cat /tmp/t0-refusal-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-refusal-falsify.$$.log
rm -f /tmp/t0-refusal-falsify.$$.log

if [ -s data/vanilla/update_shape.json ] && [ -s data/vanilla/signal.json ] \
   && [ -s data/vanilla/use_without_item.json ] && [ -s data/vanilla/set_placed_by.json ]; then
    python3 ci/refusal-ledger.py --check \
        || { echo "T0 RED — ci/refusal-ledger.py --check: a refusal population moved" >&2; t0_stage_failed; }
else
    echo "refusal-ledger: skipped — this tree has not generated the four required golden tables"
fi

# The other half of the same ledger, and the half a count cannot see. A refusal
# carries a reason; an absence carries nothing, so this derives the override set
# from the **jar** rather than from the table — a class the extraction never
# reached still appears. `DiodeBlock` had no row, no refusal and nothing that
# could say so until this ran.
#
# The falsifier needs the jar too (its shadow trees symlink the real extract),
# so both arms sit behind the same guard. ~2s with the jar warm.
if [ -s data/vanilla/set_placed_by.json ] && [ -s data/vanilla/use_without_item.json ] \
   && ls oracle-work/vanilla-*/versions/*/server-*-unsigned.jar >/dev/null 2>&1; then
    python3 ci/probe-absences-falsify.py > /tmp/t0-absences-falsify.$$.log 2>&1 \
        || { echo "T0 RED — ci/probe-absences-falsify.py: an arm did not fire" >&2
             cat /tmp/t0-absences-falsify.$$.log >&2
             t0_stage_failed; }
    tail -1 /tmp/t0-absences-falsify.$$.log
    rm -f /tmp/t0-absences-falsify.$$.log
    python3 ci/probe-absences.py --check \
        || { echo "T0 RED — ci/probe-absences.py --check: an absence is unexplained or stale" >&2; t0_stage_failed; }
else
    echo "probe-absences: skipped — needs data/vanilla and the pinned jar"
fi

# HOW MANY POSITIONS `WallBlock.updateShape` READS, ASKED OF THE JAR.
#
# `WallBlock` is 41,472 of `update_shape.json`'s 79,113 refusals — more than
# every other large class together — and the three-position extract answers all
# of them precisely because `sideUpdate` reads exactly one position past the
# subject and the changed neighbour. §83.10 established that from `javap`, once,
# by hand. It is true, it is the whole scope of the extract's largest class, and
# nothing rechecked it.
#
# Here for the same reason `ci/probe-absences.py` is: a property of the pinned
# jar that a doc quotes is a figure, and a figure with no derivation behind it
# rots at the next repin. Same jar guard, and the arithmetic half needs
# `block_props.json` and the refusal census, which the checker skips with a
# printed reason rather than silently. ~2s, both halves.
if ls oracle-work/vanilla-*/versions/*/server-*.jar >/dev/null 2>&1; then
    ./ci/wall-reads-falsify.sh > /tmp/t0-wall-falsify.$$.log 2>&1 \
        || { echo "T0 RED — ci/wall-reads-falsify.sh: an arm did not fire" >&2
             cat /tmp/t0-wall-falsify.$$.log >&2
             t0_stage_failed; }
    tail -1 /tmp/t0-wall-falsify.$$.log
    rm -f /tmp/t0-wall-falsify.$$.log
    ./ci/wall-reads-check.sh > /tmp/t0-wall-reads.$$.log 2>&1 \
        || { echo "T0 RED — ci/wall-reads-check.sh: WallBlock no longer reads one position" >&2
             cat /tmp/t0-wall-reads.$$.log >&2
             t0_stage_failed; }
    tail -2 /tmp/t0-wall-reads.$$.log
    rm -f /tmp/t0-wall-reads.$$.log
else
    echo "wall-reads: skipped — needs the pinned jar"
fi

# The third thing neither of the two above can see. `ci/refusal-ledger.py`
# counts what the generators said they could not answer; `ci/probe-absences.py`
# counts what they never said anything about; this counts what they said
# **confidently and wrongly**, because the harness is not a `ServerLevel` and
# the rule returned a plausible value without running a line.
#
# 8 rules, 82 registry blocks, 7 of them silent — `instanceof` is answered by
# the JVM against the object's class, so a `Level` subclass cannot observe the
# question, refuse it or log it. `SignBlock` is the only one vanilla announces.
# The check compares the bytecode walk against what the dumpers actually
# stamped, so a gate column that has gone inert fails here and not in a
# consumer. ~8s, 4s of it the disassembly.
if [ -s data/vanilla/set_placed_by.json ] && [ -s data/vanilla/use_without_item.json ] \
   && ls oracle-work/vanilla-*/versions/*/server-*-unsigned.jar >/dev/null 2>&1; then
    python3 ci/server-level-gates-falsify.py > /tmp/t0-gates-falsify.$$.log 2>&1 \
        || { echo "T0 RED — ci/server-level-gates-falsify.py: an arm did not fire" >&2
             cat /tmp/t0-gates-falsify.$$.log >&2
             rm -f /tmp/t0-gates-falsify.$$.log; t0_stage_failed; }
    tail -1 /tmp/t0-gates-falsify.$$.log
    rm -f /tmp/t0-gates-falsify.$$.log
    python3 ci/server-level-gates.py --check \
        || { echo "T0 RED — ci/server-level-gates.py --check: a gate, a population or a stamped column moved" >&2 || true; t0_stage_failed; }
else
    echo "server-level-gates: skipped — needs data/vanilla and the pinned jar"
fi

# `ci/step-window-audit.py` decides whether a step boundary is drawn closer
# than the reference's own answer, which is a claim about the recorder made
# by a classifier nobody else checks. Its ten cases are hermetic — synthetic
# cells, no recordings, no jar — so they cost milliseconds and run here.
#
# Seven injections at the classifier were caught cleanly when it was written
# (2026-08-09): flips ignored, flips misfiled as vanilla's, every ordinal
# reported, an intermittent kind read as stable, a payload change called
# ours, the window reader broken, and `issue_times` failing to accumulate.
# **The fourth of those was a real defect the injections found, not a
# rehearsal** — a kind present in some recordings and absent from others
# left no trace in `counts` or `pays`, both being computed only over the
# recordings where the cell exists, and read as stable.
python3 ci/step-window-audit.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/step-window-audit.py self-test" >&2
         python3 ci/step-window-audit.py --selftest 2>&1 >&2 || true; t0_stage_failed; }
# `ci/corpus-surface.py` is the corpus's denominator: how much of vanilla's
# play protocol the 88 checks can see at all. Every set it prints is scraped
# out of the pinned jar or out of our own sources, an empty set makes coverage
# look total, and a wrong coverage number is worse than none because nobody
# re-checks one. Its self-test runs here, on the same argument as the two
# above — no jar, no build, no box, a few milliseconds of in-process fixtures.
#
# **Its first version passed with a guard deleted**, because it asserted only
# that the call exited 2 and every guard in that file exits 2. It now asserts
# which guard fired, and all three of `parse_protocol`'s were falsified
# against it. Same stderr discipline as above: the happy path is silent
# because the fixtures provoke eleven refusals on purpose.
python3 ci/corpus-surface.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/corpus-surface.py self-test" >&2
         python3 ci/corpus-surface.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# `ci/serverbound-surface.py` is the other half of that denominator, and it is
# gated harder than the line above rather than more softly. `corpus-surface.py`
# reads both directions out of `GameProtocols` and reports only the clientbound
# one; this reports the serverbound one, where a gap does not surface as a wrong
# byte at all — a packet this server ignores emits nothing, so the recorder sees
# the same silence from both servers and the comparison says `match`. Every
# number it prints is therefore a reading of the tree with no measurement behind
# it and no floor available, which is exactly the situation in which a scrape
# that has quietly stopped matching is indistinguishable from good news.
#
# Two of its guards are load-bearing beyond staleness. `ACCEPTED_REASON`
# refuses in BOTH directions — a decode arm that loses its last reader stops the
# gate until somebody writes down why, and an entry for a packet that has since
# gained one is reported as unnecessary — so it cannot decay into a caveat list.
# And `strip_tests` exists because `world_send.rs`'s own unit tests hand-build
# frames with serverbound ids: without it a packet counts as dispatched because
# a test mentions it, which is this project's favourite shape of wrong.
python3 ci/serverbound-surface.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/serverbound-surface.py self-test" >&2
         python3 ci/serverbound-surface.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# `ci/vanilla-reach-census.py` is the same idea taken past the protocol: how
# much of vanilla's *tick* and of vanilla's *registries* the corpus touches at
# all, with every denominator read out of the pinned jar and every numerator
# measured off vanilla recordings. It exists because "72/92" has no
# denominator — 92 is the number of scenarios we wrote — and because a single
# coverage number with no sensitivity is worth less than several with it.
#
# Only its self-test runs here: the census itself wants the jar and the
# recordings, and neither is present in every tree. The self-test is
# in-process fixtures and milliseconds, and it fires each refusal in turn —
# including the two controls that make the game-rule gate verifier a gate
# rather than a rubber stamp.
python3 ci/vanilla-reach-census.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/vanilla-reach-census.py self-test" >&2
         python3 ci/vanilla-reach-census.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# `ci/item-registry-decompose.py` asserts an identity about vanilla rather than
# about us: that `minecraft:item` is exactly the singleton fields of
# `net.minecraft.world.item.Items` plus the members of its 30 collection
# records, `1177 + 15*16 + 15*8 == 1537`, with zero exceptions
# (`docs/PARITY-ROADMAP.md` §44.1). PARITY-ROADMAP §44 quotes that
# decomposition as the reason 1,537 is not a work queue, so it is exactly the
# kind of claim `ci/check-figures.py` exists to stop drifting — except that no
# artefact file holds it, the jar does.
#
# Unlike the census above, this one is run FOR REAL and not by self-test. It
# needs the jar, and running it costs about ten seconds, and a static assertion
# about the pinned jar cannot flap. Self-testing it against a synthetic
# `Items` would test the parser against a fixture written by the same person
# who wrote the parser, which is the weaker of the two available tests.
#
# A missing jar is a FAILURE here, by the same rule as the fixture policy at
# the bottom of this file: `RUSTMC_FIXTURES=optional` is the one way to turn it
# into a skip, and that mode already prints a different sentinel so a run with
# skips in it cannot be read as a gate result.
case "${RUSTMC_FIXTURES:-required}" in
    required)
        python3 ci/item-registry-decompose.py > /dev/null 2>&1 \
            || { echo "T0 RED — ci/item-registry-decompose.py: the item registry no longer decomposes as PARITY-ROADMAP 44.1 says it does" >&2
                 python3 ci/item-registry-decompose.py 2>&1 >&2 | tail -20 || true; t0_stage_failed; } ;;
    *) : ;;
esac

# `ci/check-sound-pitch-table.py` is the same shape and is here for the same
# reason: a static assertion about the pinned jar, which cannot flap, and
# which no artefact file holds.
#
# `tools/oracle-runner/src/pitch.rs` declares the half-open range each drawn
# sound pitch comes from, and those ranges decide whether a recorded pitch
# reads as "the draw" or survives as a number that can diverge. Too wide and a
# broken server matches; too narrow and an honest one diverges every run. They
# were read out of the jar by hand, and a constant read by hand once and never
# re-read is exactly the claim that goes stale silently the first time the
# pinned version moves. This re-derives each range from the bytecode's own
# literals and compares — a cross-check, not a restatement.
#
# Run FOR REAL rather than by self-test, on the item-registry argument: a
# self-test against a synthetic `javap` dump would test the parser against a
# fixture written by whoever wrote the parser, which is the weaker of the two
# available tests. Five `javap` calls, about two seconds.
#
# Its own red-when-broken evidence is `--falsify`, which perturbs every
# declared range wide and narrow and asserts each is refused. That does not run
# here — it rewrites `pitch.rs` in place and restores it, which is not a thing
# to do inside a gate — and it is the falsification `docs/PARITY-ROADMAP.md`
# §60 points at.
case "${RUSTMC_FIXTURES:-required}" in
    required)
        python3 ci/check-sound-pitch-table.py > /dev/null 2>&1 \
            || { echo "T0 RED — ci/check-sound-pitch-table.py: a declared sound.pitch draw range disagrees with the pinned jar" >&2
                 python3 ci/check-sound-pitch-table.py 2>&1 >&2 | tail -20; t0_stage_failed; } ;;
    *) : ;;
esac

# `ci/projection-drops.py` enumerates every field the corpus projection removes
# before comparing, and refuses one that is not marked and ranked.
#
# It is here rather than on demand because the defect it catches is invisible
# by construction and permanent once landed. `tools/oracle-runner/src/trace.rs`
# digests the bytes its decoder did **not** consume into `rest` and compares
# that exactly, so a field left unread is opaque but still live, and a field
# read without being emitted is gone from every comparison for good. The two
# are one line apart in the source. Three checks have been quietly hollowed out
# that way — `sound.pitch` decided claim 420 and had been thrown away, 607's
# greens before 13 Aug were taken through a decoder that could not see an item
# stack, and `damage_event` left raw entity ids in a digest that would have made
# the first check to observe it permanently unmatchable — and every one was
# found by accident, months late.
#
# Nothing here needs a jar, a build, a server or a recording: it reads the
# decoder, `ci/checks/projection-drops.toml` and the `observe` lists, and takes
# under a second. That is also why its falsification runs beside it rather than
# on demand — unlike the pitch table's `--falsify`, this one mutates copies in
# a `mktemp -d` and never writes to the tree, so a gate can afford to watch its
# own guard work every time.
python3 ci/projection-drops.py > /dev/null 2>&1 \
    || { echo "T0 RED — ci/projection-drops.py: the projection removes a field nobody declared or ranked" >&2
         python3 ci/projection-drops.py 2>&1 >&2 | tail -30; t0_stage_failed; }
./ci/projection-drops-falsify.sh > /dev/null 2>&1 \
    || { echo "T0 RED — ci/projection-drops-falsify.sh: the drop enumerator no longer refuses a defect it claims to catch" >&2
         ./ci/projection-drops-falsify.sh 2>&1 >&2 | tail -20; t0_stage_failed; }

# `ci/never-recorded.py` is the same discipline for the other half of the loss.
#
# `ci/projection-drops.py` above enumerates dropped **fields**, which leave an
# observation with a hole in it. `trace::is_housekeeping` refuses twelve packet
# ids before `decode` runs, and those leave nothing at all — no observation, no
# `rest` digest, not even the `packet:0xNN` fallback. The projection ledger
# records that twelve exist and cannot say what they cost, because the answer is
# a property of the pinned jar rather than of this tree: two of the twelve are
# built by no class in it, two carry nothing but a clock, and two are ordinary
# reproducible server behaviour that no check can reach.
#
# It reads the jar, so it sits behind the fixture guard with the pitch table and
# for the same reason. **The falsifier runs beside it rather than on demand**,
# despite costing about fourteen seconds against the pitch table's two: its
# central case is the only one in this repository that mutates the pinned jar,
# because the claim it protects — "no vanilla class builds this packet" — goes
# false at a version bump and not at an edit. A guard for a claim about a binary
# that is never watched against a changed binary is the check-that-cannot-fail
# this file exists to keep out.
case "${RUSTMC_FIXTURES:-required}" in
    required)
        python3 ci/never-recorded.py > /dev/null 2>&1 \
            || { echo "T0 RED — ci/never-recorded.py: a packet kind the recorder refuses is unranked, or its ranking disagrees with the pinned jar" >&2
                 python3 ci/never-recorded.py 2>&1 >&2 | tail -20; t0_stage_failed; }
        ./ci/never-recorded-falsify.sh > /dev/null 2>&1 \
            || { echo "T0 RED — ci/never-recorded-falsify.sh: the never-recorded ledger no longer refuses a defect it claims to catch" >&2
                 ./ci/never-recorded-falsify.sh 2>&1 >&2 | tail -20; t0_stage_failed; } ;;
    *) : ;;
esac

# `ci/corpus-floor.py` is a mechanism for **declaring scenarios unstable**,
# which is the single most direct route this repository has to a check that
# cannot fail. Its falsifications therefore run in T0 rather than on
# demand, on the same argument as the figure check above: a mechanism that can
# hide a defect must not be able to sit unexercised in a branch for a day.
#
# It costs seconds and needs no build, no vanilla extract and no box. When no
# measurement is on disk — a fresh clone, or the state every tree was left in
# by the 2026-08-07 disk failure — it writes a synthetic ensemble of its own
# and falsifies against that instead, so the guard does not quietly stop
# running the moment the recordings go away. That is not hypothetical: the
# 26-run selfcheck corpus this protocol's predecessor depended on did exactly
# that.
#
# **The summary line is printed on success and that is not decoration.** The
# first version of this swallowed all output on the happy path, and the run
# that gated this branch produced a T0 log in which "fifteen falsifications
# passed" and "the script was never reached" are the same absence. That is the
# defect this whole file is a catalogue of, one level up: a green you cannot
# tell from a no-op. Which route it took is printed too, because the synthetic
# route and the real-measurement route test different things.
./ci/corpus-floor-falsify.sh > /tmp/corpus-floor-falsify.$$ 2>&1 \
    || { echo "T0 RED — ci/corpus-floor-falsify.sh" >&2
         cat /tmp/corpus-floor-falsify.$$ >&2; t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/corpus-floor-falsify.$$ || true
grep -E '^(== |corpus-floor-falsify:)' /tmp/corpus-floor-falsify.$$
rm -f /tmp/corpus-floor-falsify.$$

# `ci/regiondiff-falsify.sh` — the red-when-broken evidence for
# `ci/t2-regiondiff.sh`, which until 2026-08-16 was a check that could not fail
# and had not actually run since 2026-07-22. Three injected faults, ~39s, no
# vanilla boot: a parser that mis-reads a chunk, a differ that stops counting,
# and a differ that drops one. Each must make the gate red **on the named
# assertion** — a compile error does not count as a caught fault.
#
# Its fourth arm re-harvests without `-Dmax.bg.threads=1` and is opt-in behind
# `REGIONDIFF_FALSIFY_LIVE=1`, because it costs two vanilla boots. It is the
# evidence that the gate's live half is live rather than a restatement of a
# constant, and it belongs to T2's budget, not T0's.
#
# Skipped under `RUSTMC_FIXTURES=optional`, the same opt-out
# `real_vanilla_chunks_survive_a_rewrite` uses, and for the same reason: the
# references it reads are Mojang-derived and gitignored, so a clean clone has
# no way to satisfy it. The skip is loud — a silent one here would be the exact
# defect this block exists to catch.
if [ "${RUSTMC_FIXTURES:-required}" = "optional" ]; then
    echo "t0: SKIPPING ci/regiondiff-falsify.sh — RUSTMC_FIXTURES=optional"
else
    ./ci/regiondiff-falsify.sh > /tmp/regiondiff-falsify.$$ 2>&1 \
        || { echo "T0 RED — ci/regiondiff-falsify.sh: an injected parser/differ fault went green" >&2
             cat /tmp/regiondiff-falsify.$$ >&2; t0_stage_failed; }
    grep -E '^(ok|note|REGIONDIFF-FALSIFY)' /tmp/regiondiff-falsify.$$
    rm -f /tmp/regiondiff-falsify.$$
fi

# The reference cache decides whether `ci/t2-gameplay.sh` boots vanilla at all,
# and a stale serve would be the worst failure available to this project: not a
# red, but a green that could no longer go red, taken against an artefact nobody
# re-derived. So its refusals are gated here rather than trusted.
#
# It needs no jar, no server, no lock and no recordings — a synthetic tree in a
# private REFCACHE_DIR, about two seconds — so it belongs in T0 with the other
# falsifiers rather than behind the corpus. The count is printed because
# "twenty cases passed" and "the script exited before running any" are otherwise
# the same silence, which is the defect this file catalogues.
t0_prefetched ./ci/reference-cache-falsify.sh > /tmp/t0-refcache-falsify.$$ 2>&1 \
    || { echo "T0 RED — ci/reference-cache-falsify.sh" >&2
         cat /tmp/t0-refcache-falsify.$$ >&2; t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-refcache-falsify.$$ || true
grep -E '^(== |reference-cache-falsify:)' /tmp/t0-refcache-falsify.$$
rm -f /tmp/t0-refcache-falsify.$$

# And the half the falsifier cannot see. The cache's whole purpose is to change
# what `ci/t2-gameplay.sh` DOES; a cache that answered perfectly into a harness
# that booted vanilla anyway would pass every case above and save nothing,
# silently, because the recording would still be right. So the harness is driven
# end to end against stubbed servers and the assertion is that `start` is never
# reached on a hit.
./ci/reference-cache-integration.sh > /tmp/t0-refcache-integ.$$ 2>&1 \
    || { echo "T0 RED — ci/reference-cache-integration.sh" >&2
         cat /tmp/t0-refcache-integ.$$ >&2; t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-refcache-integ.$$ || true
grep -E '^(== |reference-cache-integration:)' /tmp/t0-refcache-integ.$$
rm -f /tmp/t0-refcache-integ.$$

# --- the corpus lock's exclusion, after the build moved out of it ------------
#
# `ci/t2-gameplay.sh` builds before it queues as of 2026-08-18, so a second run
# now does real work on the box while a first one records. The lock exists
# because that must never extend to the recordings — three overlapped on
# 2026-08-06 and produced garbage — and the property is therefore two-sided:
# a waiting run MAY build and MAY NOT record.
#
# Gated here, in T0, rather than left beside the corpus, because it is the
# assertion that stops somebody reclaiming another minute by moving one more
# thing above the acquire. It boots nothing and needs no jar.
#
# **It has no hold duration to tune, deliberately.** The first run's stubbed
# recording blocks on a sentinel the test removes once it has observed what it
# needs, so the hold is exactly as long as the observation takes — on a quiet
# box and on one at load 100 alike. A `sleep N` there would have been a
# constant chosen against a load average that moves between 3 and 100 within an
# hour, and its failure mode is a case that goes red for a reason unrelated to
# the lock. That is not hypothetical: T0's own `readiness-falsify` was RED at
# load 11.8 and GREEN at 3.5 on the same tree and the same binaries. About 19 s.
#
# Its case 7 is why the 19 s is worth spending: it removes the exclusion and
# requires the same two runs to overlap, so cases 2, 3 and 6 are known to be
# able to fail. Without it this would be 19 s spent on a green that cannot go
# red.
# **The exit code is read, not just tested.** This script exits 2 for "my
# scratch tree never started, so I learned nothing" and 1 for "two recordings
# overlapped". Collapsing those into one red sentence would print
# "two corpus runs can overlap" over a tree that could not boot a stub — the
# `t0_refusals` defect class, in the file that catalogues it. `t0_run` on
# `ci-gate-stage-falsifiability` is the general form of this and should adopt
# this call site when it lands; until then it is hand-rolled here rather than
# deferred, because the wrong sentence in a gate log is what costs the time.
# `cmd` then a bare `VAR=$?` under `set -e`: see the block at `SKIPS_RC`
# above. This one mattered most of the three — its exit-2 arm is the
# environment fault that must NOT be read as "two corpus runs can overlap".
CORPUS_EXCL_RC=0
t0_prefetched ./ci/corpus-exclusion-falsify.sh > /tmp/t0-corpus-excl.$$ 2>&1 || CORPUS_EXCL_RC=$?
case "$CORPUS_EXCL_RC" in
    0) ;;
    2) echo "T0 REFUSED — ci/corpus-exclusion-falsify.sh could not run its own scratch tree." >&2
       echo "             This is an environment fault. It says NOTHING about the corpus lock." >&2
       cat /tmp/t0-corpus-excl.$$ >&2; rm -f /tmp/t0-corpus-excl.$$; exit 2 ;;
    *) echo "T0 RED — ci/corpus-exclusion-falsify.sh: two corpus runs can overlap" >&2
       cat /tmp/t0-corpus-excl.$$ >&2; t0_stage_failed ;;
esac
grep -E '^(== |corpus-exclusion-falsify:)' /tmp/t0-corpus-excl.$$
rm -f /tmp/t0-corpus-excl.$$

# --- and the instrument that will be quoted about all of it ------------------
#
# `ci/hold-phases.sh` runs inside every corpus run on the box and its output is
# what the next argument about the lock will be made from. It has to satisfy
# two things that pull opposite ways — never fail a gate, never report a number
# it did not measure — and the cheap way to get the first is to swallow
# everything, which gets you the second. Twelve cases, ~3 s, no lock and no
# jar.
./ci/hold-phases-falsify.sh > /tmp/t0-hold-phases.$$ 2>&1 \
    || { echo "T0 RED — ci/hold-phases-falsify.sh" >&2
         cat /tmp/t0-hold-phases.$$ >&2; t0_stage_failed; }
grep -E '^(== |hold-phases-falsify:)' /tmp/t0-hold-phases.$$
rm -f /tmp/t0-hold-phases.$$

# `ci/lanes-falsify.sh` was 6 cases, 4 injections and green on 2026-08-16 while
# `ci/lanes.sh` dropped 37 queued branches in 33 seconds and exited 0 — every
# case ran its drain in a lane clone the sweep had just created, so no case
# ever saw a dirty tree, and no case ever looked at what the drain returned.
# It is now 8 cases, 8 injections and closes that hole, but a sweep that is
# wired into no gate rots exactly the way the first six did: nobody re-runs it
# until the next incident. The full sweep is ~2.5 min — too slow for T0 — so
# this is the same compromise `ci/corpus-lock-falsify.sh` makes for the same
# reason: `LANES_FALSIFY_DRYRUN=1` applies every patch and runs no case, which
# proves the sweep still has a hold on `ci/lanes.sh` without paying for a
# single drain. About half a second.
LANES_FALSIFY_DRYRUN=1 ./ci/lanes-falsify.sh > /tmp/t0-lanes-falsify.$$ 2>&1 \
    || { echo "T0 RED — ci/lanes-falsify.sh (dry run)" >&2
         cat /tmp/t0-lanes-falsify.$$ >&2; t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-lanes-falsify.$$ || true
grep -E '^(lanes-falsify:|  PATCH-)' /tmp/t0-lanes-falsify.$$
rm -f /tmp/t0-lanes-falsify.$$

# **A gate does not run what nobody told it to, and until now nothing in this
# tree knew which targets those were.**
#
# The line further down is `cargo test --workspace --lib --bins`, and `--lib
# --bins` cannot build anything in a `tests/` directory. That is a deliberate
# choice with a budget behind it, but it had never been written down as a
# *decision* — only as the absence of a flag — so nobody could tell a target
# excluded on purpose from a target excluded by accident. On 2026-08-10 an agent
# wrote an integration test, watched T0 come back GREEN, and the green was the
# test binary never having been built. Its own deliverable was a check that
# could not fail.
#
# `ci/checks/test-targets.toml` names every cargo target in the workspace and
# puts each in a bucket that says which gate runs it and why.
# `ci/check-test-targets.py` enumerates the tree with `cargo metadata` and
# reddens on a target nobody classified, on a manifest entry with no target
# behind it, and — the part that makes it a control rather than a comment — on
# the manifest and T0's **actual argv** disagreeing about what T0 runs.
#
# **Since 2026-08-16 it also asks whether a target that runs could have
# failed**, because on that day twenty of the 110 could not: nine lib/bin
# binaries with no `#[test]` in them, nine doctest targets with no runnable
# example, and two `#[ignore]`d instruments. Nine of the twenty were in T0's
# own `t0-unit` bucket, so this line was printing a clean count over a gate
# that declared 22 checks and could fail 13 of them. `EXECUTES NO ASSERTION`
# is the refusal, `[[compile_only]]` in the manifest is the one sanctioned
# exception and its membership rule is enforced rather than described, and
# `[assertions]` ratchets the count so it cannot fall back silently.
#
# **It is deliberately placed above the test suite rather than beside it.** A
# gate line after a suite that exits non-zero under `set -e` never runs at all,
# and that has bitten this file twice. Everything this check needs is manifest
# reading and `cargo metadata`, which compiles nothing, so there is no cost
# argument for putting it later.
python3 ci/check-test-targets.py \
    || { echo "T0 RED — the test-target manifest and the tree disagree" >&2 || true; t0_stage_failed; }

# --- the persistence stage's applicability ledger ---------------------------
#
# `ci/handoff.sh` stage 7 runs the four persistence gates only when the branch
# touches a path declared in `ci/checks/persistence-paths.toml`. That decision
# is invisible in a gate log — both answers produce a plausible line — so both
# the ledger and the decider are checked here.
#
# `--check` refuses a declared prefix nothing in the tree matches, a prefix
# declared twice, a `gates` entry naming a script `ci/t2-persistence.sh` does
# not run, and a list shorter than its own floor. It caught the first of those
# on its own first run: the ledger shipped naming
# `crates/rustmc-world/src/region.rs`, which does not exist.
#
# `ci/persistence-paths-falsify.sh` is the red-when-broken evidence for the
# decider itself — every declared prefix must make the stage apply, four
# realistic non-persistence branches must not, and each of the four refusals
# above must fire. No build, no jar, no lock, about two seconds.
python3 ci/persistence-applies.py --check \
    || { echo "T0 RED — the persistence-paths ledger is broken" >&2 || true; t0_stage_failed; }
./ci/persistence-paths-falsify.sh > /tmp/t0-persistence-paths.$$ 2>&1 \
    || { echo "T0 RED — ci/persistence-paths-falsify.sh"; cat /tmp/t0-persistence-paths.$$; \
         rm -f /tmp/t0-persistence-paths.$$; t0_stage_failed; }
tail -1 /tmp/t0-persistence-paths.$$
rm -f /tmp/t0-persistence-paths.$$

# --- and the stage driver's own red ----------------------------------------
#
# `ci/t2-persistence.sh` is the check whose green stage 7 reads, and "it runs
# the scripts and prints GREEN" is not evidence that it prints anything else.
# `ci/t2-persistence-falsify.sh` builds a lab of fake gates and requires a RED
# from each of the three ways one can go wrong — no sentinel, a sentinel with
# a bad exit status, and **a control that did not fire**.
#
# The third is the one that cannot be seen from a passing run and is the
# reason the driver asserts on a string rather than on `$?`: a control arm
# exits 0 when the control WORKED, so a status-based driver reports a control
# that has silently stopped controlling as a pass.
#
# No cargo, no jar, no lock, no JVM, about a second.
./ci/t2-persistence-falsify.sh > /tmp/t0-t2persist-falsify.$$ 2>&1 \
    || { echo "T0 RED — ci/t2-persistence-falsify.sh"; cat /tmp/t0-t2persist-falsify.$$; \
         rm -f /tmp/t0-t2persist-falsify.$$; t0_stage_failed; }
tail -1 /tmp/t0-t2persist-falsify.$$
rm -f /tmp/t0-t2persist-falsify.$$

# **And the same question asked of that check: what would make it come back
# red?** Until 2026-08-16 the honest answer for half its subject was "nothing".
# It re-evaluated `ci/t0.sh`'s argv and refused on a disagreement, and it had
# never opened `ci/t1.sh` — so the 88 targets the manifest marks `runs_in =
# "t1"` were covered by twelve prose `reason` fields and by no line of code.
# Narrowing `ci/t1.sh` would have taken every golden vector and every
# integration test out of every gate in the project, with this line still
# printing its reassuring count.
#
# `ci/check-test-targets-falsify.py` builds a five-target cargo workspace under
# `mktemp -d`, copies the real checker into it, and injects forty cases:
# a target added and wired to nothing, T1 narrowed, T0 narrowed, T0 widened, a
# retirement that lies in each direction, four ways a selector line can become
# invisible — and, for the assertion floor, a target that runs and cannot fail,
# that same target moved into the compile-only category to make the arithmetic
# work, an assertion deleted, one assertion of several deleted, an `#[ignore]`
# added to the last test in a target, and five ways the static model's
# calibration against a real suite log can be wrong. Each must be refused **by
# name**, and every repair must be accepted, because a rule with no green form
# only teaches people to route round it. ~2 s, no build, no jar, no box.
python3 ci/check-test-targets-falsify.py > /tmp/t0-test-targets-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-test-targets-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-test-targets-falsify.$$.log >&2
         t0_stage_failed; }
tail -1 /tmp/t0-test-targets-falsify.$$.log
rm -f /tmp/t0-test-targets-falsify.$$.log

# --- the check that cannot go red -----------------------------------------
#
# This project's central defect, and until 2026-08-13 it had no gate: an
# instrument that measures its own input, or a verdict whose green cannot be
# turned red. A readiness proof grepping for `listening on`, which this server
# prints *before* it binds; `0 of 539 stacks opaque`, guaranteed because the
# step feeding the decoder wrote a literal `0`; a `window` counter coupled to
# zero checks; `ACCEPT_TELEPORTATION` decoded into a field read by nothing.
# Every parity figure published here rests on its checks not being of this
# kind. See `docs/PARITY-ROADMAP.md` §27 and the HARD RULE at the top of
# `docs/AGENT-BRIEF.md`.
#
# **Both selftests run before both gates**, and that ordering is the point of
# this whole block: a gate whose own controls have not been exercised is
# exactly what it is here to refuse.
python3 ci/corpus-falsifiable.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/corpus-falsifiable.py self-test" >&2
         python3 ci/corpus-falsifiable.py --selftest 2>&1 >&2 || true; t0_stage_failed; }
python3 ci/liveness-proofs.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/liveness-proofs.py self-test" >&2
         python3 ci/liveness-proofs.py --selftest 2>&1 >&2 || true; t0_stage_failed; }

# **The packet-family classification is declared, measured, and shown able to
# go red.** `trace::is_housekeeping` used to drop twelve clientbound packet
# types at RECORD time with only a paragraph defending the list, which is this
# project's central defect class in the one form no falsifiability gate here
# could see: not a check that cannot fail, but a question that cannot be asked.
# `--check` reads `trace::FAMILIES`, `compare.rs` and the committed census, and
# needs no recordings — same split as `corpus-falsifiable.py` above and for the
# same reason. `--falsify` is its red-when-broken evidence and runs beside it.
python3 ci/ambient-falsify.py --check \
    || { echo "T0 RED — a packet family is dropped without a measured declaration" >&2 || true; t0_stage_failed; }
python3 ci/ambient-falsify.py --falsify > /dev/null 2>&1 \
    || { echo "T0 RED — ci/ambient-falsify.py cannot catch its own injections" >&2
         python3 ci/ambient-falsify.py --falsify 2>&1 >&2 || true; t0_stage_failed; }

# `--check` reads the committed ledger, not recordings: T0 runs where no
# server has ever been started, and a gate that skipped itself when recordings
# were absent would be a check that cannot fail, manufactured by our own hand.
# The ledger is produced by `--sweep`, which needs a trace pair but no boot and
# **not the corpus lock** — see the HARD RULE in `docs/AGENT-BRIEF.md` for when
# to re-run it.
python3 ci/corpus-falsifiable.py --check \
    || { echo "T0 RED — a gameplay check carries no evidence it can go red" >&2 || true; t0_stage_failed; }

# **`RUSTMC_CORPUS_SHARDS` may only default to a shard count that was proven
# not to move a verdict** (2026-09-24, branch `corpus-parallel`).
# `ci/t2-gameplay.sh` can record the corpus as N concurrent shards; whether that
# changes what it measures is a measurement, recorded in
# `ci/checks/corpus-shard-equivalence.toml` by
# `ci/corpus-shard-equivalence.py compare` (serial, a serial control, and the
# sharded run). `--check` refuses a default that no controlled EQUIVALENT run
# covers; `--selftest` feeds the comparator ten constructed recordings — four
# shard effects it must name, three floors it must not charge, two broken
# denominators it must refuse — through the real `oracle-runner compare`.
# **Every corpus harness starts its servers the way the gate does.** The subset
# copy drifted from `ci/t2-gameplay.sh` twice, both silently: a dead `time set`
# confirmation and a missing `server.pid` that made `ci/vanilla-server.sh
# console` drop every game rule sent to our server. Text rules over every
# `ci/*.sh`; `--selftest` shows each refusing a constructed drift, and the
# checker flags main's pre-fix subset script on all three rules.
python3 ci/harness-startup-agree.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/harness-startup-agree.py self-test" >&2
         python3 ci/harness-startup-agree.py --selftest >&2 || true; t0_stage_failed; }
python3 ci/harness-startup-agree.py \
    || { echo "T0 RED — a corpus harness starts a server differently from ci/t2-gameplay.sh" >&2 || true; t0_stage_failed; }
python3 ci/corpus-shard-equivalence.py --check \
    || { echo "T0 RED — ci/t2-gameplay.sh defaults to a shard count nothing proved equivalent" >&2 || true; t0_stage_failed; }
cargo build --locked -q -p oracle-runner
python3 ci/corpus-shard-equivalence.py --selftest --bin target/debug/oracle-runner \
    > /tmp/t0-shard-eq.$$.log 2>&1 \
    || { echo "T0 RED — ci/corpus-shard-equivalence.py self-test" >&2
         cat /tmp/t0-shard-eq.$$.log >&2; rm -f /tmp/t0-shard-eq.$$.log; t0_stage_failed; }
tail -1 /tmp/t0-shard-eq.$$.log; rm -f /tmp/t0-shard-eq.$$.log

# **And the survival corpus, which had no ledger and no hook here at all until
# 2026-09-05.**
#
# `ci/checks/survival` is the second corpus: `ci/t2-gameplay-survival.sh` runs
# it on a survival substrate to grade the four mechanics creative cannot reach
# — damage, health, hunger, mining progress and block drops all sit behind an
# early return above the send (`ci/substrate-gate-audit.py` reads the offsets
# out of the jar). Every one of those scenarios' verdicts was ungraded by this
# gate: nothing read a survival ledger, so no survival check had ever been
# shown able to go red, and a second corpus with no falsifiability gate is a
# second corpus whose greens mean nothing.
#
# **The condition is derived from the scenario directory, not from a flag.**
# A hook that skipped itself when the ledger was absent would be a check that
# cannot fail, manufactured by our own hand — the exact reasoning in the block
# above about why `--check` reads the committed ledger rather than recordings.
# So: scenarios present and no ledger is a refusal, and the only other way out
# is deleting the corpus, which is a visible act.
if [ -d ci/checks/survival ] \
   && [ "$(find ci/checks/survival -name '*.toml' | wc -l)" -gt 0 ]; then
    if [ ! -f ci/checks/survival-falsifiable.toml ]; then
        echo "T0 RED — ci/checks/survival has scenarios and no falsifiability ledger." >&2
        echo "         Sweep it (needs a survival recording pair, no lock, no boot):" >&2
        echo "           ci/t2-gameplay-survival.sh          # if you have no pair yet" >&2
        echo "           ci/corpus-falsifiable.py --sweep --corpus survival \\" >&2
        echo "               --bin target/release/oracle-runner \\" >&2
        echo "               oracle-work/gameplay/survival/{vanilla,rustmc}.json" >&2
        t0_stage_failed
    fi
    python3 ci/corpus-falsifiable.py --check --corpus survival \
        || { echo "T0 RED — a survival check carries no evidence it can go red" >&2; t0_stage_failed; }

    # **And the survival pass's one load-bearing assertion, which nothing ran.**
    #
    # `ci/survival-substrate-assert.py` is the check that the substrate gates
    # actually opened. Its own header says it is "the one assertion in
    # `ci/t2-gameplay-survival.sh` whose absence is invisible" — if vanilla
    # behaved as it does in creative, every scenario still runs, every packet
    # still arrives, the comparison is still well-formed, and nothing
    # downstream catches it. It ships with a falsifier, six cases, 0.5s, no
    # server and no recordings.
    #
    # **`ci/survival-substrate-assert-falsify.sh` was run by nothing** — not
    # T0, not `ci/handoff.sh`, not the chain. That is the shape the paragraph
    # further down is about: `ci/unfalsifiable-falsify.py` had been red for as
    # long as nobody ran it end to end. A guard nobody runs is a guard whose
    # green nobody has seen.
    ./ci/survival-substrate-assert-falsify.sh > /dev/null 2>&1 \
        || { echo "T0 RED — ci/survival-substrate-assert-falsify.sh" >&2
             ./ci/survival-substrate-assert-falsify.sh 2>&1 >&2; t0_stage_failed; }
fi

# The positive-control battery must not be reachable in a shipped server.
#
# `crates/rustmc-server/src/sabotage.rs` replaces seven subsystems with the
# not-yet-written versions they grew out of, so that a gameplay check can be
# asked what would have reddened it. Every entry point compiles to a constant
# `false` without `--features corpus-sabotage`, and the feature is not in any
# default set. **That is asserted here rather than trusted**, because a control
# that can be switched on in production is not a control, it is a backdoor, and
# the switch is one word in a manifest.
#
# Two arms, because either alone is passable. The manifest must not list the
# feature under `default`, and no crate in the workspace may name it as a
# dependency feature — a transitive `features = ["corpus-sabotage"]` on a
# dependency edge would arm it in a build whose own manifest looks clean.
if grep -qE '^default *=.*corpus-sabotage' crates/rustmc-server/Cargo.toml; then
    echo "T0 RED — corpus-sabotage is in rustmc-server's default features" >&2
    t0_stage_failed
fi
# What each gameplay check can be made to say, as a generated artefact rather
# than a paragraph. `--check` needs no recordings: it reads the committed
# ledger and refuses on drift — a scenario with no row, a row whose scenario
# has changed since its reading, a row for a scenario that is gone, and the
# bucket counts against floors pinned in `ci/corpus-grades.py`.
#
# **The floors are in the script and not in the ledger, deliberately.** A
# ledger carrying its own floor is a ratchet comparing a number to itself,
# which is the defect this whole instrument was built after finding one line
# down in `ci/corpus-falsifiable.py`.
python3 ci/corpus-grades.py --selftest > /dev/null 2>&1 \
    || { echo "T0 RED — ci/corpus-grades.py self-test" >&2
         python3 ci/corpus-grades.py --selftest 2>&1 >&2; t0_stage_failed; }
python3 ci/corpus-grades.py --check \
    || { echo "T0 RED — the gameplay grades ledger has drifted from the corpus" >&2; t0_stage_failed; }

# The five defects found in the falsifiability machinery on 2026-08-18, each
# reconstructed and shown caught. Needs no recordings, no server and not the
# corpus lock, which is the point: an audit of the auditors that could only run
# behind a forty-minute lock would not be run, and every one of those five read
# as green on the day it was found.
#
# The sharpest is the last: `ci/unfalsifiable-falsify.py` — the falsifier for
# unfalsifiable checks — asserted a tolerance its own gate had withdrawn, and
# had been red for as long as nobody ran it end to end. Nothing in T0 does run
# it; this runs the part that needs no recordings.
python3 ci/corpus-machinery-falsify.py > /dev/null 2>&1 \
    || { echo "T0 RED — a guard over the falsifiability machinery stopped guarding" >&2
         python3 ci/corpus-machinery-falsify.py 2>&1 >&2; t0_stage_failed; }

stray_sabotage="$(find . -name Cargo.toml -not -path './target/*' \
    -not -path './crates/rustmc-server/Cargo.toml' -exec grep -ln 'corpus-sabotage' {} + || true)"
if [ -n "$stray_sabotage" ]; then
    echo "T0 RED — a manifest outside rustmc-server names the corpus-sabotage feature:" >&2
    echo "$stray_sabotage" >&2
    t0_stage_failed
fi

# The readiness half. Discovers every wait loop in `ci/*.sh` and runs each
# registered predicate against a log a NOT-ready subject produces.
python3 ci/liveness-proofs.py --check \
    || { echo "T0 RED — a readiness proof cannot say \"not ready\"" >&2 || true; t0_stage_failed; }

# The third member of the same family, added 2026-08-18 with the chunk-send
# path. `corpus-falsifiable.py` refuses a check no mutation of its subject can
# move; this refuses a packet KIND that reaches no recording at all, which the
# former cannot see. When the projection filtered a kind before the trace was
# written, no mutation moves anything, so the scenario reads as tier 1 — the
# same signature as a scenario that measures nothing. The two are
# indistinguishable to that gate by construction.
#
# Selftest first, same ordering and same reason as the pair above: seven cases
# including a positive control and a reconstruction of the real shipped
# instance (LEVEL_CHUNK_WITH_LIGHT dropped while §31 and §32 were written
# against it). About 1.5 s, no server, no recordings.
./ci/chunk-send-unrecorded-falsify.sh > /dev/null 2>&1 \
    || { echo "T0 RED — ci/chunk-send-unrecorded.py self-test" >&2
         ./ci/chunk-send-unrecorded-falsify.sh 2>&1 >&2; t0_stage_failed; }

python3 ci/chunk-send-unrecorded.py \
    || { echo "T0 RED — a chunk-send packet kind reaches no recording and is not ledgered" >&2; t0_stage_failed; }

# The guard-of-the-guards. `ci/unfalsifiable-falsify.py` reconstructs three
# shipped defects against the real comparator and real recordings and asserts
# each is refused and each repair accepted — the bar `docs/AGENT-BRIEF.md`
# holds both `--check` blocks above to. It was wired into NOTHING until
# 2026-08-17: `docs/PARITY-ROADMAP.md` §27.6 said this class is "asked
# mechanically, on every commit, and cannot be forgotten" while the one script
# proving it had never once run in a gate, and its case 1d had been silently
# wrong since 2026-08-13 for exactly that reason — see the branch that added
# this block for the fix and the finding.
#
# It cannot run unconditionally: unlike the fixture-only checks above, it
# needs `target/release/oracle-runner` and a real vanilla/subject recording
# pair, neither of which a from-scratch checkout has, and forcing a release
# build here would make T0 the opposite of fast. So it runs when a tree
# already carries them — every tree that has recorded a real corpus, which is
# every tree actively touching either guard — and names the skip rather than
# passing silently on a fresh one, so the skip itself is auditable instead of
# being a second door the class can walk back in through.
#
# **And the binary is THIS tree's, built here, not whatever the last run left.**
# It used to be run as found. In a gate directory `target/release/oracle-runner`
# is the previous branch's build — T0 runs before anything rebuilds it — so
# the falsifier graded this branch's scenarios with another branch's parser
# and comparator. `fix-flappers` (2026-10-04, gate-fix-flappers-20261004-
# 141458.log) went red on exactly that: its 086 declares an egg component
# field the stale binary did not know, every `compare` refused the corpus
# with a TOML parse error, and the sweep wrote no ledger. A branch that changed
# what `compare` decides would have been graded by the old decision instead,
# silently. So when the pair is here the binary is (re)built first — an
# incremental release build of one crate — and a failed build is a red.
UFZ_BIN="target/release/oracle-runner"
UFZ_VAN="oracle-work/gameplay/vanilla.json"
UFZ_SUB="oracle-work/gameplay/rustmc.json"
if [ -s "$UFZ_VAN" ] && [ -s "$UFZ_SUB" ] && ! cargo build --locked -q --release -p oracle-runner; then
    echo "T0 RED — could not build $UFZ_BIN for ci/unfalsifiable-falsify.py" >&2
    t0_stage_failed
elif [ -x "$UFZ_BIN" ] && [ -s "$UFZ_VAN" ] && [ -s "$UFZ_SUB" ]; then
    python3 ci/unfalsifiable-falsify.py --bin "$UFZ_BIN" "$UFZ_VAN" "$UFZ_SUB" \
        > /tmp/t0-unfalsifiable-falsify.$$.log 2>&1 \
        || { echo "T0 RED — ci/unfalsifiable-falsify.py: corpus-falsifiable.py or liveness-proofs.py no longer catches a real defect" >&2
             cat /tmp/t0-unfalsifiable-falsify.$$.log >&2
             t0_stage_failed; }
    tail -1 /tmp/t0-unfalsifiable-falsify.$$.log
    rm -f /tmp/t0-unfalsifiable-falsify.$$.log
else
    echo "T0: skipping ci/unfalsifiable-falsify.py — no $UFZ_BIN or no recording pair yet (run ci/t2-gameplay.sh once)"
fi

# And the half that boots a server, because the line above cannot.
#
# `liveness-proofs.py` runs each predicate against a **fixture** — text held to
# be what a not-ready subject prints. Nothing in it starts a server, so the
# fixtures are a claim, and on 2026-08-13 that claim became load-bearing in a
# new way: with the announcement moved past the bind, `grep -q 'listening on'`
# is *sound*, and the registry's negative fixtures say so by containing no
# such line. Re-introduce the early print in `main.rs` and every fixture here
# turns into fiction while this gate stays green — which is precisely the
# failure mode the block above exists to refuse, arriving through the one door
# it does not watch.
#
# So this boots the real binary into four not-ready states, captures what it
# prints, and runs each harness script's readiness line — lifted verbatim from
# the registry — against every capture. It shares the debug artefacts
# `cargo test` builds a few lines down, and the boots are a flat world at
# radius 2. Measured on the Threadripper, 2026-08-13, with those artefacts
# already present: **22.1 s wall, 6.2 CPU-seconds** — almost all of it two
# server boots, so it costs this gate wall time and next to no core.
./ci/readiness-falsify.sh > /tmp/t0-readiness-falsify.$$.log 2>&1 \
    || { echo "T0 RED — a readiness proof's fixtures are not what the server prints" >&2
         cat /tmp/t0-readiness-falsify.$$.log >&2
         t0_stage_failed; }
grep -E "^BOX-SKIP" /tmp/t0-readiness-falsify.$$.log || true
tail -2 /tmp/t0-readiness-falsify.$$.log
rm -f /tmp/t0-readiness-falsify.$$.log

# --- the environment's own falsifiers ---------------------------------------
#
# Three repairs on the `boxcopy` branch each ship with the thing that can say
# they are real, and all three run here rather than being run once by hand:
#
#   ci/box-env-falsify.sh      the repo-dependent checks still go RED in a copy
#                              with no `.git` — cases 2, 5 and 9 force the real
#                              red rather than showing a green
#   ci/lockfile-falsify.sh     a cargo run in a clean worktree leaves it clean,
#                              and `--locked` refuses instead of rewriting
#   ci/shared-refs-falsify.sh  nothing removes a shared reference through a link
#
# They cost seconds, need no build, no jar, no server and no corpus lock, so
# they belong in T0 and not below it. Each one carries its own control — a case
# that must still FAIL if the thing under test stops being able to fail —
# because a falsifier which has quietly become unconditional is the defect this
# project has twenty-one instances of, and adding three unguarded ones to close
# an instance of it would be a poor trade.
for f in ci/box-env-falsify.sh ci/lockfile-falsify.sh ci/shared-refs-falsify.sh \
         ci/t0-ordering-falsify.sh; do
    "./$f" > "/tmp/t0-$(basename "$f" .sh).$$.log" 2>&1 \
        || { echo "T0 RED — $f: the environment repair it gates is not holding" >&2
             cat "/tmp/t0-$(basename "$f" .sh).$$.log" >&2
             t0_stage_failed; }
    grep -E "^BOX-SKIP" "/tmp/t0-$(basename "$f" .sh).$$.log" || true
    tail -1 "/tmp/t0-$(basename "$f" .sh).$$.log"
    rm -f "/tmp/t0-$(basename "$f" .sh).$$.log"
done

# `cargo fmt --check` with no `--all`, measured rather than assumed on
# 2026-08-07: the root manifest is **virtual**, so cargo-fmt has no current
# package to narrow to and formats every workspace member. A misformatted
# `tools/beta/src/main.rs` in a replica of this layout took it to exit 1.
# `--all` would be a no-op here and is deliberately not added — a flag that
# changes nothing reads as coverage somebody has checked for.
#
# What it does **not** reach is the path dependency at `../plugins`, which is
# not a workspace member. That is what the second block below is for.
# --- the cheap block is over; stop here if any of it failed ----------------
#
# Reported as a count and not just a verdict, because "T0 RED" over a wall of
# output is what made a single environment fault look like a parity failure.
if [ "$T0_STAGES_FAILED" != 0 ]; then
    echo "T0 RED — $T0_STAGES_FAILED stage(s) failed above; not starting the build stages." >&2
    echo "         Every stage before this line ran, so the list above is complete." >&2
    exit 1  # T0-CHEAP-BLOCK-GATE — the one permitted exit in the cheap block
fi


cargo fmt --check
cargo clippy --locked --all-targets -q -- -D warnings
cargo check --locked -q --all-targets
# Unit tests, and only the unit tests.
#
# T0 ran fmt, clippy and check and **no tests at all**, which is how three
# `block_column` unit tests shipped broken and stayed broken while every
# pre-commit gate came back green. The fix that broke them was itself verified
# against a server world, so nothing else caught it either.
#
# `--lib --bins` and no further: the golden-vector gates live in T1 because
# they need the vanilla extract and take minutes. These need neither and run in
# about nine seconds, which is inside T0's budget and buys the one thing T0
# could not previously see — whether the code still does what its own tests
# say.
#
# **`--bins` is not decoration.** With `--lib` alone this gate ran no test in
# `tools/oracle-runner` at all, because it is a binary crate: 19 tests,
# invisible to every green gate since the harness was written, one of them
# failing. It asserted that the cursor for the DOWN face lay on the *x* face,
# because `Direction`'s ordinal groups the axes Y, Z, X and the test used the
# pair index as the coordinate index. The gate cannot be the reason a test is
# never run.
#
# The sentinel below is read by `ci/check-test-targets.py`, which recomputes
# what this argv actually selects and reddens if `ci/checks/test-targets.toml`
# claims anything else. Narrowing this line without updating the manifest is a
# red gate, which is the property the manifest exists to have. If you add
# another `cargo test` line for this workspace, give it a sentinel too — an
# unmarked line is invisible to the checker and its targets will read as unrun.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q --workspace --lib --bins

# --- the one integration test T0 must reach, named explicitly ---------------
#
# `--lib --bins` above cannot run anything in a `tests/` directory. That is
# correct for every other integration test in this workspace — the golden-vector
# gates need the vanilla extract and take minutes, and two of them
# (`carved_chunks_match_vanilla`, `surface_chunks_match_vanilla`) are standing
# reds adjudicated by signature in T1. Adding `--tests` here would drag all of
# that into a one-minute gate and turn two known reds into a red T0.
#
# So `session_joint` is named. It is an integration test because it drives
# `keep_alive_loop` through the crate's public API, exactly as `status::listen`
# does, and that is the joint it exists to witness; it needs `data/vanilla` but
# costs 3.3 s, which is inside T0's budget.
#
# **This line exists because the test would otherwise have been the very defect
# it was written to close.** Six arms, six negative controls, a full write-up —
# and `--lib --bins` would never have executed one of them. Checked, not
# assumed: the first T0 run after the test landed came back GREEN without the
# binary being built. That is `--lib` and `tools/oracle-runner` again, in a
# tree that already carries the comment about it fifteen lines up.
#
# It is placed *here*, immediately after the line above and before the plugin
# subshell, for a reason worth keeping: a gate line after a suite that exits
# non-zero under `set -e` never runs at all.
#
# If you add another integration test that gates a production wiring rather
# than a golden vector, name it here too. The rule is not "integration tests
# are T1"; it is "tests that need minutes and the extract are T1".
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test session_joint

# --- three integration targets that are cheap enough to gate here -----------
#
# The audit behind `ci/checks/test-targets.toml` timed all 88 targets the line
# above cannot reach. These three are the ones with no recorded reason to be
# out, no dependence on the 3.8 GB vanilla extract, and a measured cost of
# **0.3 s for all three together** (Threadripper, 2026-08-10, box contended, so
# that is an upper bound). They gate production wirings rather than golden
# vectors: the plugin storage contract, the plugin tick lock, and the positions
# random ticks are drawn at. `ci/plugin-hook-falsify.sh` already drives
# `plugin_tick_lock` by name, which is somebody having decided it is
# load-bearing without a gate ever running it.
#
# The rest of the 88 stay in T1 and the manifest says why, one bucket at a time.
# The short version: the two standing reds are 480 s of the 830 s and would make
# T0 permanently red; the golden vectors need the extract; three targets are
# instruments that assert nothing on purpose.
#
# `entity_move` is deliberately NOT here despite costing 1.8 s.
# `crates/rustmc-server/tests/entity_move.rs:33` records a decision that it
# belongs to T1, and an audit does not overturn a written decision as a
# side effect of counting.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test plugin_storage --test plugin_tick_lock --test random_tick_pos --test golden_chunk_send_order
# `sky_golden` — 513 vectors from vanilla's own AttributeTrackSampler, compared
# as raw float bits with no tolerance, in milliseconds. Named here rather than
# left to T1 because of how it failed the first time: all three of its cases
# skipped for a missing `data/vanilla/timelines.json` and it reported three
# passes. The table is in the derived required set now and
# `ci/sky-falsify.sh` case 0 asserts the skip is announced, but the third
# guard is running it in the gate every branch runs — a golden that only T1
# executes is a golden most branches never see. `ci/checks/test-targets.toml`
# bucket `t0-sky-golden`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test sky_golden

# --- the three player-joint witnesses --------------------------------------
#
# Same rule as `session_joint` above, applied to the three targets that gate
# the player wiring: they drive `keep_alive_loop` through the crate's public
# API and need `data/vanilla` but not the 3.8 GB extract.
#
#   move_correction        the speed budget and the correction teleport
#   fall_damage_wire       a fall, the health it costs, and the set_health
#   fall_damage_agreement  fall.rs against falldamage.rs, 20,505 cells
#
# **`move_correction` was in the tree and in no bucket**, which is a T0 red on
# `ci/check-test-targets.py` by itself: `origin/player-movement` added the
# target and not the manifest row, and the manifest exists precisely because
# a test target nobody named is a test nobody runs. It is named here with the
# two this branch adds rather than separately, because all three are the same
# bucket and the same argument.
#
# Measured together on the Threadripper, 2026-09-05, release:
# `fall_damage_wire` 2.3 s, `fall_damage_agreement` 0.03 s,
# `move_correction` 0.03 s — 2.4 s, inside T0's budget and dominated by the
# four full play-loop runs `fall_damage_wire` drives.
# **ON ONE LINE, AND THAT IS NOT STYLE.** `ci/check-test-targets.py` reads
# exactly the ONE line after the marker above and tokenises it, so a shell
# line-continuation splits the selector in half: it refused this with
# `unrecognised token '\\'` (ORACLE rule 30 — an unrecognised knob is not a
# turned one) and, had it guessed instead, the manifest would have claimed a
# T0 run for `fall_damage_agreement` that the checker never saw. The refusal
# is the instrument working; the fix is the line, not the checker.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test move_correction --test fall_damage_wire --test fall_damage_agreement

# --- the mob AI's ground pathfinding, over a grid level ---------------------
#
# `mobs-goals` transcribed WalkNodeEvaluator, PathFinder and
# GroundPathNavigation; these two targets drive them over
# `rustmc_entity::testing::GridLevel` with no extract and no `data/vanilla`,
# 0.03 s together. The jar goldens for the same classes are deferred by name
# inside the files (a `Mob` in a `Level` is needed to take them); the two that
# could be taken without a server — the A* and Mth.atan2 — are in the
# golden-vectors bucket with the rest. `ci/checks/test-targets.toml`,
# bucket `mobs-ground`.
# `--locked` is not decoration and `ci/lockfile-falsify.sh` case 5 enumerates
# every cargo stage in this file to say so: a cargo invocation without it may
# rewrite `Cargo.lock`, a tracked file, mid-gate — and case 1 of that script
# demonstrates exactly that, `rc=0, and git now sees a dirty tree`. This line
# arrived from main without it.
#
# **Nothing may come between the sentinel below and the command.**
# `ci/check-test-targets.py` reads the line immediately after
# `# check-test-targets:` and REFUSES if it is not a `cargo test` invocation —
# which is what putting this paragraph there did, and it is right to: a
# sentinel that scanned forward for the next cargo line would keep pointing at
# something after an edit inserted a different one.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-entity --test ground_path --test navigation

# --- natural spawning's tables and its decision log ------------------------
#
# `natural-spawning` transcribed `NaturalSpawner` into
# `crates/rustmc-entity/src/spawn/`. These two targets are the whole of what
# holds it: `spawn_tables` reads the three generated tables
# (`biome_spawners.json`, `spawn_placements.json`, `entity_spawn_facts.json`)
# and refuses a registered spawn predicate that is in neither TRANSCRIBED nor
# PENDING, which is the version-bump tripwire; `spawn_decisions` drives the
# whole decision over a synthetic chunk with a seeded generator and compares
# the RNG draw counter against a count re-derived from the event log — two
# readings of one run, so a draw taken on the wrong branch moves one and not
# the other, and a pinned copy of the log could not have said that.
#
# They need `data/vanilla` and not the 3.8 GB extract, which is the
# `loot-interpreter` shape below. 0.10 s together (Threadripper 2026-09-05).
# There is no jar golden and there cannot be one yet — a live capture of
# natural spawns needs a mob in the world and a corpus substrate that is not
# peaceful; `docs/cold-start/natural-spawning.md` names what that takes.
# `ci/checks/test-targets.toml`, bucket `spawn-tables`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-entity --test spawn_tables --test spawn_decisions --test spawn_state

# --- the loot interpreter, whose whole claim lives in its `tests/` ----------
#
# `crates/rustmc-loot` is 813 lines of interpreter and every statement it makes
# about vanilla is made by one of these three targets: `golden_loot` replays the
# vectors `ci/loot-golden.sh` recorded from vanilla rolling its own 1,355 tables,
# `block_dispatch` asks whether every block in the game gets an answer or a named
# refusal, and `falsify_loot` is the evidence the first two can go red. Under
# `--lib --bins` none of them is built, so the crate's lib target passing is a
# green that measures nothing — which is this ledger's founding defect, and the
# crate landed carrying it.
#
# They cost 1.26 s of test time together (0.25 + 0.13 + 0.88, Threadripper
# 2026-09-05, `cargo test --release -p rustmc-loot`) and need `data/vanilla` but
# not the 3.8 GB extract, which is the `session_joint` shape exactly.
# `ci/checks/test-targets.toml`, bucket `loot-interpreter`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-loot --test golden_loot --test block_dispatch --test falsify_loot --test entity_dispatch

# --- the weather wiring, which no unit test in that module can see -----------
#
# `crate::weather` is thirty unit tests against a caller-supplied level and a
# scripted generator, and not one of them can answer whether the console, the
# tick loop and the join path are actually CONNECTED to it — the defect class
# `ci/corpus-falsifiable.py`'s header enumerates. `weather_console` drives a
# real fifo through `crate::console::start` and asserts the next tick's
# broadcast; it was shown red twice before it was shown green (delete the
# `Some("weather")` console arm, and flip the rain ramp's sign).
#
# It is here rather than in T1 for the `t0-wiring` bucket's own reason: no
# vanilla extract, no `data/vanilla`, 0.01 s green. Its 10 s deadline is
# reached only on failure, and reaching it IS the failure — the assertion
# carries the state it saw. `ci/checks/test-targets.toml`, bucket
# `weather-console`.
#
# It must stay a separate `--test` from the three above, and separately a
# single test in its own file: `weather::shared()` and
# `weather::level_random()` are process-wide singletons, the same reason
# `plugin_tick_lock` is alone in its binary.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-server --test weather_console

# --- BlockItem.canPlace's canSurvive gate -----------------------------------
#
# `crates/rustmc-behavior/src/place_survive.rs` is the first half of
# `BlockItem.canPlace`, which this server has never asked: a torch placed
# against air was placed. The target replays `data/vanilla/item_survive.json`
# in full — 6,600 states over 79 fully specified neighbourhoods at two light
# levels, 640,532 cells for the ported families — asserts the disagreement
# count is zero and the covered-cell count exactly, names the 34 classes it
# does not answer, and carries `a_wrong_rule_is_caught`, which makes a
# `BaseTorchBlock` survive on air and requires the sweep to catch it.
#
# 1.4 s; needs `data/vanilla` and not the 3.8 GB extract, the `session_joint`
# and `loot-interpreter` shape exactly. Under `--lib --bins` none of it is
# built, so the crate's lib target passing would be a green measuring nothing.
# `ci/checks/test-targets.toml`, bucket `place-survive`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-behavior --test golden_item_survive

# --- vanilla's whole command tree through our own encoder -------------------
#
# `ci/commands-golden.sh` gates the bytes of the tree THIS server registers,
# which is three literal nodes; `crates/rustmc-server/src/commandtree.rs`'s
# header lists what a tree that small cannot distinguish. This one hands the
# same encoder vanilla's entire dispatcher — 2,085 nodes, 1,084 argument nodes
# over 55 parsers, 112 redirects — and requires vanilla's own bytes back, at
# all five permission levels. 0.09 s; the fixture is generated, not the 3.8 GB
# extract. `ci/checks/test-targets.toml`, bucket `command-tree-vanilla`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-server --test command_tree_vanilla

# --- what an argument PARSE consumes ----------------------------------------
#
# The three gates above are all about the tree a client is SHOWN. None of them
# says anything about what happens when a player types a command into it, and
# the failure there is silent: a parser that takes one character too many
# leaves the NEXT node reading the wrong input, so a present, correctly-spelled
# command with a working executor comes back "unknown command". So this one
# compares the CURSOR, over 770 (parser, input) rows recorded from vanilla's own
# `ArgumentType.parse`, and carries a positive control that requires a
# deliberately one-character-wrong parser to be caught on every row that has a
# character left to take. 0.01 s; the fixture is generated.
# `ci/checks/test-targets.toml`, bucket `command-parse-vanilla`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-server --test command_parse_vanilla

# --- the answer to every command that does not run ---------------------------
#
# One reply shape whose reach is the whole unported surface: every typo, every
# root this server has not ported, and every command a player lacks permission
# for. `ci/commands-golden.sh` runs vanilla's own `Commands.finishParsing` over
# eleven failing inputs at five permission levels against vanilla's copy of OUR
# tree, with a recording `CommandSource`, and this compares the bytes. Carries a
# positive control that moves the cursor by one — the quantity every part of the
# second component is derived from — and requires every row to be caught.
# 0.01 s; the fixture is generated. `ci/checks/test-targets.toml`, bucket
# `command-failure-vanilla`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test -q --locked -p rustmc-server --test command_failure_vanilla

# `crate::cmdexec`'s replies against the components vanilla's own code builds
# (the command's message lambda by reflection, its exception type, a real
# `sendFailure`). Reads the same generated `commands_tree.json`; hundredths of a
# second. `ci/checks/test-targets.toml`, bucket `cmdexec-vanilla`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test cmdexec_vanilla

# --- the section light comparison, and the wiring it grades ----------------
#
# Until 2026-08-18 every chunk packet this server sent carried
# `LightData::default()` — four empty bitsets and two empty array lists, which
# classifies **0 of the 26** light sections vanilla puts in that payload — and
# no gate anywhere would have gone red for it. `docs/PARITY-ROADMAP.md` 75.
#
# Both targets are named here on the `session_joint` rule — "tests that need
# minutes and the extract are T1", not "integration tests are T1". Measured on
# the Threadripper, 2026-08-18: `light_golden` 1.78 s, `section_light_wired`
# 7.75 s.
#
# They grade two different things and neither substitutes for the other:
#
#   * `light_golden` compares our classification against vanilla's own, dumped
#     from `ClientboundLightUpdatePacketData` by `ci/light-golden.sh` — 3,276
#     sections over seven worlds. It is the check that can go red.
#   * `section_light_wired` drives the server's own `World::chunk` and reads
#     the light back off the encoded packet. Without it, an engine that is
#     exact against vanilla and never runs in the server would gate green —
#     which is the `ACCEPT_TELEPORTATION` defect with a better test suite
#     attached.
#
# `section_light_wired` needs the 3.8 GB extract and skips itself with a
# printed reason when it is absent, which is why it is here rather than in the
# bucket above whose members need none.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test light_golden --test section_light_wired

# --- and the proof that the comparison above can fail ----------------------
#
# `ci/light-falsify.sh` breaks each of the rules `light_golden` enforces, one
# at a time, and requires it to notice: the mask-bit/section-Y conflation, the
# 3x3x3 stored-section cube reduced to a column, light leaking into unstored
# sections, `getOpacity` without its `max(1)`, `isEmpty` ignoring
# `defaultValue`, an untrimmed BitSet, and the registered history divergence
# losing its control. Case 1 is a positive control on the unmodified tree.
#
# The full suite rebuilds per case and costs about five minutes, so T0 gates
# the **dry run**, which applies every injection, confirms each still changes
# its file, and runs no cargo. That is the same split, for the same reason, as
# `CORPUS_LOCK_FALSIFY_DRYRUN`: this suite does not rot by giving wrong
# verdicts, it rots when a refactor moves a line a `sed` was matching and the
# injection silently applies to nothing — after which the case reports "red"
# for a tree that was never patched. Invisible to the full run, obvious here.
# --- the incremental engine: the queues, and the edit path -----------------
#
# `light_golden` above is structurally blind to the decrease queue: every case
# it reads reaches the engine through `propagateLightSources`, which enqueues
# nothing but increases, so a port with no decrease queue at all passes all of
# it. Three injections proved that rather than arguing it — see
# `ci/checks/test-targets.toml`, bucket `t0-incremental-light`.
#
#   * `light_incremental_golden` — vanilla's own algorithm over the same
#     vectors, 6,084/6,084 sections.
#   * `light_incremental_mutation` — an already-lit world edited and drained
#     again, 2,340/2,340 over five edited worlds. The only thing in the tree
#     that reaches `propagateDecrease` or `removeSourcesBelow`.
#
#   * `live_light_edit` — the SERVER's light, not the engine's. It builds a
#     real `World`, sends a chunk, and only then edits, which is the order
#     that reddens on the defect it was written for: `World::light` was a
#     `OnceLock` and every player edit after the first chunk send was
#     invisible on the wire. 0 of 1,916,928 nibbles against the graded
#     post-edit instrument, plus the `light_update` masks and the filter
#     clear. It is here rather than in T1 because the wire now carries light
#     by default on every branch and every other light target in the tree
#     drives the engine directly — none of them construct a `World`, so a
#     regression in the live path is invisible to all of them.
#
# `light_harvest` — the comparison against a real server's saved nibbles in
# three dimensions — is T1, not here: decoding the three harvests is ~5,500
# chunks and costs 112 s in this gate's debug build against 1.7 s for the two
# above. `ci/checks/test-targets.toml`, bucket `t1-light-harvest`. So is
# `golden_chunk_light`, which needs a capture that cannot be committed.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test light_incremental_golden --test light_incremental_mutation --test live_light_edit

# The dry run of the incremental falsifier, on the same split and for the same
# reason as the one below: this suite rots when a refactor moves a line a
# `sed` was matching, after which a case reports "red" for a tree that was
# never patched.
LIGHT_INCREMENTAL_FALSIFY_DRYRUN=1 ./ci/light-incremental-falsify.sh \
    > /tmp/t0-light-incremental-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/light-incremental-falsify.sh: an injection no longer applies" >&2
         cat /tmp/t0-light-incremental-falsify.$$.log >&2; exit 1; }
rm -f /tmp/t0-light-incremental-falsify.$$.log

LIGHT_FALSIFY_DRYRUN=1 ./ci/light-falsify.sh > /tmp/t0-light-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/light-falsify.sh: an injection no longer applies" >&2
         cat /tmp/t0-light-falsify.$$.log >&2
         rm -f /tmp/t0-light-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-light-falsify.$$.log
rm -f /tmp/t0-light-falsify.$$.log

# --- every villager trade in the game, and what a villager can offer today ---
#
# In 26.2 a trade is a datapack registry entry, not a Java `ItemListing`, so the
# whole of "which trades does a level-3 farmer draw from" is 388 listings, 68
# trade sets and 73 tag pools that `ci/villager-trade-golden.sh` extracts. The
# unit tests in `crates/rustmc-entity/src/trading/` gate the arithmetic and
# never see one row of that; this target is the only thing in the tree that
# reads it, and its counts come from vanilla's registry cross-checked against
# Mojang's own data generator rather than from the reader. It also runs
# `VillagerTrade.getOffer` over all 388 and pins the split — 327 exact offers,
# 61 refused BY NAME, 0 plain nulls — so a subsystem going missing cannot read
# as a villager who legitimately has no such trade. 0.05 s; the fixture is
# generated, not the 3.8 GB extract. `ci/checks/test-targets.toml`, bucket
# `villager-trades`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-entity --test villager_trade_tables

# --- the enchantment registry, the table's offers and the anvil's cost ------
#
# Three targets, and the first is the one that has to be in a gate rather than
# in a worktree: `enchantment_registry_order` compares the order of
# `data/vanilla/enchantments.json` against the entry ids of
# `minecraft:enchantment` decoded out of `data/vanilla/registries.bin` — bytes
# a real vanilla server sent. The dumper's first answer for that order was
# `VanillaRegistries.createLookup()`'s iteration, which is the BOOTSTRAP's
# registration order and not the live registry's, and it would have put a wrong
# `enchantClue` id on the wire for all 43 with nothing else in the tree looking.
#
# `enchanting_offers` and `anvil_cost` are unit gates over pure functions:
# `EnchantmentMenu.slotsChanged`'s draw order (asserted through a RandomSource
# that records every call and its bound, not just its answer) and
# `AnvilMenu.createResult`'s six refusal branches, each by name. Both read
# `data/vanilla/enchantments.json` and neither needs the 3.8 GB extract, which
# is the `session_joint` shape.
#
# 40 assertions, 0.22 s warm together (Threadripper, 2026-09-06,
# `cargo test -q -p rustmc-server --test enchantment_registry_order --test
# enchanting_offers --test anvil_cost`, debug profile, 26.0 s from cold).
# `workstation_menus` drives those two through the menus that call them —
# stonecutter, anvil, grindstone, the loom's pattern ids against the capture,
# the creative slot's length-prefixed damage — over the same table and
# `data/vanilla/item_props.json`.
# `ci/checks/test-targets.toml`, bucket `enchantments`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test enchantment_registry_order --test enchanting_offers --test anvil_cost --test workstation_menus

# --- the recipe table, and which of two matching recipes wins ---------------
#
# `recipe_match_golden` replays 3,378 vectors that `ci/recipe-match-golden.sh`
# recorded from vanilla's own
# `RecipeMap.getRecipesFor(type, input, null).findFirst()`, over grids built
# from vanilla's `Recipe.placementInfo()`. It is here rather than in T1 for the
# reason `t0-wiring` gives — the rule is "tests that need minutes and the
# extract are T1", not "integration tests are T1" — and it needs neither: 1.0 s,
# against a generated fixture, no `oracle-work`.
#
# What it gates that nothing under `--lib` can: **256 of its 1,561 positive
# grids are won by a recipe other than the one they were built from.**
# `getRecipeFor` is `findFirst` over a bucket ordered by recipe id, so the
# winner is a property of the whole table's sort order and has no local
# symptom — every recipe can be right and the answer wrong. A unit test written
# from the same reading of `recipes.json` agrees with itself about all of them.
#
# `recipes_table` is on the same line because it is the half the golden
# structurally cannot reach: the golden's grids come from `PlacementInfo`, so a
# `material_count` of `exactly(1)` yields a two-slot grid and no vector can ever
# present two materials. `ci/recipe-match-falsify.sh` measured exactly that —
# that mutation SURVIVES the golden alone. 0.5 s.
# `ci/checks/test-targets.toml`, bucket `recipe-vectors`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test recipe_match_golden --test recipes_table

# --- the border a joining client actually receives ---------------------------
#
# The one target in this tree that gates a WIRING rather than a transcription.
# `crates/rustmc-server/src/worldborder.rs` was byte-exact against vanilla for
# a day while nothing in the server sent any of it, and that is invisible by
# inspection: a client with no border and a client with a default-size border
# look the same. This drives `world_send::enter_world` and reads back the
# frames, so it fails if the packet is absent, if it is in the wrong place in
# the join sequence, or if the border it carries is not the one this process
# serves.
#
# T0 rather than T1, unlike the rest of the world-systems goldens, because it
# reads only `crates/rustmc-server/golden/world_border.json`, which is
# committed: a clean clone with no `oracle-work` runs it. 0.00 s.
# `ci/checks/test-targets.toml`, bucket `world-systems-wiring`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test join_border

# --- the world border a joining client is actually sent -------------------
#
# `ci/checks/test-targets.toml`, bucket `world-systems-wiring`, has argued
# since 2026-09-05 that `join_border` runs here — "T0 rather than T1, unlike
# its `t1-world-systems-golden` neighbours, because it reads only
# `crates/rustmc-server/golden/world_border.json`, which is **committed**" —
# and no selector reached it, so `ci/check-test-targets.py` said MANIFEST SAYS
# T0 RUNS ... AND IT DOES NOT and T0 was red on its own manifest.
#
# That is this project's own defect class, one door along: the bucket's
# paragraph is a claim about coverage that nothing in the tree delivered, and
# a target under `tests/` is invisible to `--lib --bins`. The manifest was
# right about where it belongs; the selector was missing.
#
# 0.00 s, one test, no fixture beyond the committed golden.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test join_border

# --- the packets a client receives when it changes dimension ---------------
#
# The same gap as the line above, one system along: `src/dimension.rs` was
# green for a day with no `respawn` id in `rustmc-proto` to send anything
# through. Needs no fixture — `change_dimension` writes into a Vec and the
# frames are read back — so a clean clone runs it. 0.07 s, four tests.
# `ci/checks/test-targets.toml`, bucket `dimension-travel-wiring`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test dimension_travel_wire

# --- the explosion, against vanilla running its own ------------------------
#
# `ci/explosion-ray-golden.sh` reflectively invokes the PRIVATE
# `ServerExplosion.calculateExplodedPositions()` on a `ServerExplosion` and a
# `ServerLevel` subclass allocated without their constructors, over twelve
# worlds; `ci/explosion-wire-golden.sh` runs
# `ClientboundExplodePacket.STREAM_CODEC.encode` on a real
# `RegistryFriendlyByteBuf`. So these two compare the port against vanilla's
# own output rather than against a second reading of the same bytecode: 5,395
# positions in `java.util.HashSet` order, the same list after `Util.shuffle`,
# and 289 bytes of packet. 0.02 s; both fixtures are generated.
# `ci/checks/test-targets.toml`, bucket `explosion-goldens`.
# check-test-targets: the next line is T0's workspace test selector.
cargo test --locked -q -p rustmc-server --test golden_explosion_ray --test golden_explosion_wire --test golden_explosion_entity

# --- and whether those three goldens can still go red ------------------------
#
# `ci/explosion-falsify.sh` declares an outcome for each of seventeen
# mutations of `crates/rustmc-server/src/explosion.rs` — thirteen CAUGHT and
# four MISSED, the four being precision constants whose effect is ~1e-7 blocks
# against an instrument that floors to whole ones. The full sweep rebuilds
# seventeen times and takes about ten minutes, so it is not here.
#
# What IS here is its dry run: apply all seventeen to a COPY and run none, in
# about a second. That is the arm with the hole in it — a patch that has
# stopped matching means the sweep ran nothing for that case, whether or not
# the cases that did run were green — and it is the same split
# `CORPUS_LOCK_FALSIFY_DRYRUN` uses for the same reason.
EXPLOSION_FALSIFY_DRYRUN=1 ./ci/explosion-falsify.sh

# --- the sibling plugin contract, which nothing gated ----------------------
#
# `crates/rustmc-server` depends on `grit-plugin-api` by **path**, at
# `../plugins/crates/grit-plugin-api` (see the workspace `Cargo.toml` for why it
# is a path and not a copy). Cargo compiles a path dependency as a dependency:
# it builds the library, and nothing else. Measured on 2026-08-07 against a
# replica of this layout, everything above stayed green while:
#
#   * a `#[cfg(test)]` module in the path dependency did not compile;
#   * `tests/contract.rs` in the path dependency did not compile;
#   * the path dependency's sources were misformatted;
#   * the path dependency tripped a `-D warnings` clippy lint.
#
# fmt rc=0, clippy rc=0, check rc=0, test rc=0 in every one of the four. That is
# 83 real tests in `grit-plugin-api` and `grit-protect` — including the whole
# capability-negotiation contract this server refuses plugins by — outside every
# gate in either repository, because the plugins repo has no T0 of its own and
# this one could not see past the library.
#
# So T0 gates the contract it builds against. Measured cost on the box, warm:
# 0.2 s for all four, against T0's one-minute budget. `plugins/host-proof` is a
# separate workspace on purpose (it links the real server) and is deliberately
# NOT here: it would make this loop depend on the thing it is testing.
#
# A missing checkout is an error and never a skip, the same rule and for the
# same reason as `ci/t0-remote.sh`: without it the tree above cannot have built,
# so a skip here could only ever hide something.
#
# One consequence, stated because it is shared state: every tree on the box
# points at the same `../plugins`, so two concurrent T0 runs now take the same
# `plugins/target` lock and serialise there. Warm that is 0.2 s; cold it is one
# small crate. `ci/t0-remote.sh` already rsyncs identical content into that
# directory from every tree for the same reason, so this adds no new sharing.
PLUGINS_DIR="${GRIT_PLUGINS_DIR:-../plugins}"
[ -f "$PLUGINS_DIR/crates/grit-plugin-api/Cargo.toml" ] || {
    echo "T0 RED — no grit-plugins checkout at $PLUGINS_DIR" >&2
    echo "         crates/rustmc-server depends on it by path; it cannot have built" >&2
    exit 1; }
# **Every command below carries its own `|| exit 1`, and that is not belt and
# braces.** A subshell on the left of `||` is "part of a `||` list", so POSIX
# and bash both suspend `set -e` inside it: written without them, a red
# `cargo fmt --check` would not stop the subshell and the subshell's status
# would be the *last* command's. Verified on 2026-08-07 — a deliberately
# misformatted file gave exit 0 through `( cd d; cargo fmt --check; cargo test )
# || ...` and exit 1 through the form below. Adding a check to this gate in the
# shape of a check that cannot fail is the one way to make it worse.
(
    cd "$PLUGINS_DIR" || exit 1
    cargo fmt --check || exit 1
    cargo clippy --locked --all-targets -q -- -D warnings || exit 1
    cargo check --locked -q --all-targets || exit 1
    # This is the OTHER workspace, and the marker is what makes that explicit to
    # `ci/check-test-targets.py` rather than merely obvious to a human reading
    # the `cd` four lines up. Note what it is bare: this line runs the sibling's
    # integration tests AND its doctests, while the line for our own workspace
    # runs neither. The contract crate is better gated here than the server that
    # depends on it (`docs/CI.md`, and `docs/DETERMINISM.md` §24).
    # check-test-targets: the next line tests a DIFFERENT workspace, not this one.
    cargo test --locked -q --workspace || exit 1
) || { echo "T0 RED — the grit-plugins contract at $PLUGINS_DIR is red" >&2; exit 1; }
# The mob family: three checks and their falsifiers.
#
# All three are cheap and none of them starts a server or takes the corpus
# lock. They gate three different things that were each true and unrecorded
# until 2026-08-19:
#
#   * `ci/mob-tick-census.py --check` — the jar-derived size of the mob
#     surface, against this crate's `EntityKind` constants. It exists because
#     `ci/unported-inventory.py --open` reports ONE entity-behaviour gap and
#     that is a fact about the crate's refusal markers, not about how much mob
#     is missing: a marker needs a host line, and a subsystem with no module
#     cannot carry markers proportional to its size.
#   * `ci/mob-observability.py --check` — every check that can put a mob in
#     the world has an entry in the blindness ledger. Without it, a new mob
#     check can observe two announcement kinds, go green, and be read as mob
#     parity while the mob does nothing.
#   * `ci/mob-substrate.py --check` — the world `ci/t2-gameplay.sh` boots has
#     not moved. Every mob finding on this branch is conditional on it, and a
#     substrate that changed silently would leave the findings standing.
#
# Each falsifier is wired directly after its check, on the pattern the rest of
# this file uses, because a check whose falsifier is optional is a check.
python3 ci/mob-tick-census.py --check \
    || { echo "T0 RED — ci/mob-tick-census.py: a pinned mob-surface figure moved" >&2; exit 1; }
./ci/mob-tick-census-falsify.sh > /tmp/t0-mob-census-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/mob-tick-census-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-mob-census-falsify.$$.log >&2
         rm -f /tmp/t0-mob-census-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-mob-census-falsify.$$.log
rm -f /tmp/t0-mob-census-falsify.$$.log

python3 ci/mob-observability.py --check \
    || { echo "T0 RED — ci/mob-observability.py: a mob-bearing check has no blindness-ledger entry" >&2; exit 1; }
./ci/mob-observability-falsify.sh > /tmp/t0-mob-obs-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/mob-observability-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-mob-obs-falsify.$$.log >&2
         rm -f /tmp/t0-mob-obs-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-mob-obs-falsify.$$.log
rm -f /tmp/t0-mob-obs-falsify.$$.log

python3 ci/mob-substrate.py --check \
    || { echo "T0 RED — ci/mob-substrate.py: the corpus substrate moved and the mob findings are conditional on it" >&2; exit 1; }
./ci/mob-substrate-falsify.sh > /tmp/t0-mob-substrate-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/mob-substrate-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-mob-substrate-falsify.$$.log >&2
         rm -f /tmp/t0-mob-substrate-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-mob-substrate-falsify.$$.log
rm -f /tmp/t0-mob-substrate-falsify.$$.log

# The order-keyed family: five checks and their falsifiers.
#
# They gate one subject — a behaviour whose output depends on a SEQUENCE
# rather than on state, which is the class of defect no state comparison can
# see. None of them starts a server or takes the corpus lock.
#
#   * `ci/entity-id-allocator.py --check` — vanilla's entity id allocator
#     against ours. It is a parity subject and not an implementation detail
#     because `Mob.serverAiStep` gates its goal phase on
#     `(tickCount + getId()) % 2`, so the id's PARITY is behaviour-visible.
#   * `ci/order-keyed-census.py --check` — the eleven sites in the jar whose
#     order is keyed on something other than state, each in one of four
#     order classes, with the concrete container derived from the bytecode.
#   * `ci/fastutil-open-hash-walk.py --check` — the ten inputs to
#     `Int2ObjectOpenHashMap`'s walk order, read out of the shipped fastutil's
#     bytecode. §103 ports that walk, and this is the file that says which
#     inputs the port reproduced rather than asserting that it did. Its
#     falsifier's arm 10 breaks the port and requires the Rust replay test over
#     948 measured histories to go red.
#   * `ci/entity-broadcast-order.py --check` — the reachable one.
#     `ChunkMap.tick` walks a plain `Int2ObjectOpenHashMap`, so per-entity
#     broadcast order is a hash of the id values; ours is ascending, and the
#     two differ at {1,2}. The check gates the derivation and the recorded
#     difference, NOT a corpus verdict — there is no corpus verdict, and
#     `ci/order-selfconsistency.py` is where that is written down.
#   * `ci/order-selfconsistency.py --check` — for each of those orders, does
#     vanilla agree with ITSELF, over N = 20 distinct recordings. Without it
#     the census is a list of things to worry about with no way to tell a
#     requirement from noise.
#   * `ci/order-blind.py --check` — the RATCHET. Nothing in the corpus can
#     grade a per-tick entity order today, and a divergence nobody can see is
#     one nobody has to fix. This refuses the day a check crosses the line
#     while the divergence is still open, so it lands as the thing to do
#     rather than as a regression from whichever scenario happened to see it.
#
# `ci/idhash-determinism.sh` is deliberately NOT here: it compiles and runs
# five cold JVMs and its answer is a property of HotSpot rather than of this
# tree, so it belongs in the branch's evidence rather than in every gate.

t0_prefetched python3 ci/entity-id-allocator.py --check > /dev/null \
    || { echo "T0 RED — ci/entity-id-allocator.py: the entity id allocator moved against its pin" >&2; exit 1; }
t0_prefetched ./ci/entity-id-allocator-falsify.sh > /tmp/t0-entid-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/entity-id-allocator-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-entid-falsify.$$.log >&2
         rm -f /tmp/t0-entid-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-entid-falsify.$$.log
rm -f /tmp/t0-entid-falsify.$$.log

t0_prefetched python3 ci/order-keyed-census.py --check > /dev/null \
    || { echo "T0 RED — ci/order-keyed-census.py: the order-keyed surface moved against its pin" >&2; exit 1; }
t0_prefetched ./ci/order-keyed-census-falsify.sh > /tmp/t0-orderkeyed-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/order-keyed-census-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-orderkeyed-falsify.$$.log >&2
         rm -f /tmp/t0-orderkeyed-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-orderkeyed-falsify.$$.log
rm -f /tmp/t0-orderkeyed-falsify.$$.log

python3 ci/fastutil-open-hash-walk.py --check > /dev/null \
    || { echo "T0 RED — ci/fastutil-open-hash-walk.py: one of the ten inputs to the ported walk moved against its pin" >&2; exit 1; }
./ci/fastutil-open-hash-walk-falsify.sh > /tmp/t0-fuwalk-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/fastutil-open-hash-walk-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-fuwalk-falsify.$$.log >&2
         rm -f /tmp/t0-fuwalk-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-fuwalk-falsify.$$.log
rm -f /tmp/t0-fuwalk-falsify.$$.log

python3 ci/entity-broadcast-order.py --check > /dev/null \
    || { echo "T0 RED — ci/entity-broadcast-order.py: the broadcast-order derivation moved against its pin" >&2; exit 1; }
./ci/entity-broadcast-order-falsify.sh > /tmp/t0-bcorder-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/entity-broadcast-order-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-bcorder-falsify.$$.log >&2
         rm -f /tmp/t0-bcorder-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-bcorder-falsify.$$.log
rm -f /tmp/t0-bcorder-falsify.$$.log

# ANY NON-ZERO IS A RED, and the exit-2 arm that stood here for one day is
# gone with the reason for it. On 2026-09-05 this check's population was every
# ~/rustmc*/oracle-work/gameplay*/vanilla.json on the box, so any lane
# recording a vanilla run reddened T0 on every branch — it was red on
# unmodified main b6237f29c and not one pinned verdict had moved. Declaring the
# population by sha256 fixed which recordings count and not whether they exist,
# and the next morning a declared recording was already gone and this line
# exited 2 on a tree that had touched none of it.
#
# The recordings are this check's FIXTURE, not its subject: what a branch can
# move is the proxy code and ci/checks/entity-broadcast-order.json. So the
# fixture is committed (ci/checks/order-selfconsistency-corpus.json) and
# --check never looks at the box. There is nothing left for an environment
# refusal to be about, and an exit code the script cannot emit is a branch
# nothing takes.
python3 ci/order-selfconsistency.py --check > /dev/null || {
    echo "T0 RED — ci/order-selfconsistency.py: a self-consistency verdict moved against its pin" >&2
    python3 ci/order-selfconsistency.py --check 2>&1 >/dev/null | head -20 >&2
    exit 1; }
./ci/order-selfconsistency-falsify.sh > /tmp/t0-oselfc-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/order-selfconsistency-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-oselfc-falsify.$$.log >&2
         rm -f /tmp/t0-oselfc-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-oselfc-falsify.$$.log
rm -f /tmp/t0-oselfc-falsify.$$.log

python3 ci/order-blind.py --check > /dev/null \
    || { echo "T0 RED — ci/order-blind.py: a check can now grade a per-tick entity order while our order still differs from vanilla's" >&2; exit 1; }
./ci/order-blind-falsify.sh > /tmp/t0-orderblind-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/order-blind-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-orderblind-falsify.$$.log >&2
         rm -f /tmp/t0-orderblind-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-orderblind-falsify.$$.log
rm -f /tmp/t0-orderblind-falsify.$$.log

python3 ci/marker-density.py --check \
    || { echo "T0 RED — ci/marker-density.py: a refusal-marker census no longer tracks lines written, which is the bound ci/unported-inventory.py's output is read under" >&2; exit 1; }
./ci/marker-density-falsify.sh > /tmp/t0-marker-density-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/marker-density-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-marker-density-falsify.$$.log >&2
         rm -f /tmp/t0-marker-density-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-marker-density-falsify.$$.log
rm -f /tmp/t0-marker-density-falsify.$$.log

python3 ci/mob-projection-search.py --check \
    || { echo "T0 RED — ci/mob-projection-search.py: a projection's shape changed; vanilla's agreement with itself on the mob announcement is what any mob check rests on" >&2; exit 1; }
./ci/mob-projection-search-falsify.sh > /tmp/t0-mob-proj-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/mob-projection-search-falsify.sh: an injected fault went unrefused" >&2
         cat /tmp/t0-mob-proj-falsify.$$.log >&2
         rm -f /tmp/t0-mob-proj-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-mob-proj-falsify.$$.log
rm -f /tmp/t0-mob-proj-falsify.$$.log


# The scheduling invariants that are NOT in this repository. gate-chain.sh,
# chain-keeper.sh and preflight.sh decide every merge this project makes, live
# under /home/sayer/grit, and are therefore un-gateable by anything else here.
#
# The paths checked are ABSOLUTE, and `threadripper` is an ssh alias to
# localhost, so the gate copy on the box resolves the same three files as the
# canonical checkout and this check has teeth in both. Worth stating because
# the obvious assumption is the opposite one: `ci/t0-remote.sh` rsyncs
# `--exclude .git` and these files are not in the tree, so it LOOKS like the
# box cannot see them. It can, because they were never being read out of the
# tree in the first place.
#
# The SKIP path is therefore not the box case. It is any tree on a machine
# where the orchestrator is not installed, where reporting a green nobody
# earned would be the worse answer — so it names each absent file and does not
# print GREEN at all.
python3 ci/check-queue-scheduling.py \
    || { echo "T0 RED — an out-of-tree scheduling invariant is broken" >&2; exit 1; }

# ...and the checker itself must be able to go red. It is the newest member of
# the class this repo keeps re-finding — a check nobody has watched fail — and
# it is one whose subject is absent on the box, so a vacuous pass there would
# look identical to a real one.
python3 ci/check-queue-scheduling.py --selftest > /tmp/t0-qsched-selftest.$$.log 2>&1 \
    || { echo "T0 RED — ci/check-queue-scheduling.py --selftest failed" >&2
         cat /tmp/t0-qsched-selftest.$$.log >&2; rm -f /tmp/t0-qsched-selftest.$$.log; exit 1; }
rm -f /tmp/t0-qsched-selftest.$$.log

# THE CLICK BUTTON AXIS, READ OUT OF THE JAR RATHER THAN OUT OF THIS TREE.
#
# `ci/click-surface.py --check` parses `AbstractContainerMenu.doClick`'s own
# guards and refuses when they stop saying what `docs/PARITY-ROADMAP.md` §81's
# table says: the `SWAP` arm's hotbar bound and its `SLOT_OFFHAND` literal, the
# `button == 0 || button == 1` guard that `PICKUP` and `QUICK_MOVE` SHARE, and
# `CLONE` never loading the button at all.
#
# It is here rather than in a unit test because it reads the pinned jar, and it
# is worth the seconds because the loop it breaks is otherwise closed: the
# corpus asks what `oracle-runner` can send, `oracle-runner` sends what
# `crates/rustmc-server/src/click.rs` names, and those two agreeing is a fact
# about this repository. Vanilla is the only party downstream of neither.
python3 ci/click-surface.py --check \
    || { echo "T0 RED — the jar's click button axis no longer matches PARITY-ROADMAP §81" >&2; exit 1; }

# And the control for it. Five doctored copies of the real javap output that
# `--check` must refuse, plus the real one it must accept — because five
# refusals alone are also satisfied by a checker that refuses everything.
t0_prefetched python3 ci/click-surface-falsify.py > /tmp/t0-click-surface-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/click-surface-falsify.py: a doctored jar went green" >&2
         cat /tmp/t0-click-surface-falsify.$$.log >&2; rm -f /tmp/t0-click-surface-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-click-surface-falsify.$$.log; rm -f /tmp/t0-click-surface-falsify.$$.log

# THE TWO SERVERS THE GAMEPLAY CORPUS COMPARES MUST BE ON THE SAME DIFFICULTY,
# AND UNTIL 2026-09-05 THEY WERE NOT.
#
# `ci/t2-gameplay.sh` booted vanilla with `difficulty=peaceful` and booted this
# server with no difficulty argument at all. At PEACEFUL, `SpawnEggItem.useOn`
# returns `InteractionResult.FAIL` before it reads the clicked position
# (`EntityType.canSpawn`, offsets 13-31), so a monster scenario is graded
# against vanilla silence while the subject, at its own default of `easy`,
# spawns the mob. Three lanes met it and all three wrote around it: the ten mob
# scenarios on `mob-in-world`, `mob-combat` and `mobs-in-world-2` are pigs,
# cows, sheep, chickens, a bat and two golems, and not one is a monster.
#
# Costs under two seconds and needs no server, no box and no corpus lock: the
# predicate runs against fixture trees and the symmetry is checked textually,
# with the shipped defect injected back into a copy of each harness script.
./ci/gameplay-difficulty-falsify.sh > /tmp/t0-gameplay-difficulty.$$.log 2>&1 \
    || { echo "T0 RED — ci/gameplay-difficulty-falsify.sh: the corpus's two servers can differ in difficulty" >&2
         cat /tmp/t0-gameplay-difficulty.$$.log >&2; rm -f /tmp/t0-gameplay-difficulty.$$.log; exit 1; }
tail -1 /tmp/t0-gameplay-difficulty.$$.log; rm -f /tmp/t0-gameplay-difficulty.$$.log

# AND THE GAME RULES, WHICH WAS THE SAME DEFECT ONE FUNCTION AWAY.
#
# `ci/t2-gameplay.sh` quiets both servers — the four substrate rules go to
# vanilla and then, over `--console`, to this server, because this server's
# `random_tick_speed` default is vanilla's 3. `ci/t2-gameplay-subset.sh` quieted
# only vanilla: it passed no `--console` to `rustmc-server`, so it could not
# have sent them at all, and the subject random-ticked, advanced its clock and
# cycled its weather through every subset run this project has taken. That is
# the asymmetry the subset script's own header forbids.
#
# The list was also written out twice, which is how the two could drift, and
# the reference cache's `harness` component hashed only `ci/t2-gameplay.sh`.
# One array, one file, and the key covers it.
./ci/gameplay-substrate-falsify.sh > /tmp/t0-gameplay-substrate.$$.log 2>&1 \
    || { echo "T0 RED — ci/gameplay-substrate-falsify.sh: the corpus's two servers can be quieted differently" >&2
         cat /tmp/t0-gameplay-substrate.$$.log >&2; rm -f /tmp/t0-gameplay-substrate.$$.log; exit 1; }
tail -1 /tmp/t0-gameplay-substrate.$$.log; rm -f /tmp/t0-gameplay-substrate.$$.log

# --- the floor's vocabulary is tree-only and was silently absent -----------
# `ci/corpus-floor-report.py` prints NO VOCABULARY and withdraws the floor's
# quotability on every corpus run, and `ci/corpus-floor.py check_floor_file`
# appends `floor.vocabulary` to `bad` — but only in a tree that happens to hold
# a saved measurement of the ensemble. §50.5 called that "conditionally armed,
# and nothing says so". Whether the file has the key is a fact about the file,
# so it belongs in a tree-only gate where it is armed always.
python3 ci/floor-stamp-vocabulary.py --check ci/checks/gameplay-floor.toml \
    || { echo "T0 RED — ci/checks/gameplay-floor.toml declares no vocabulary; the floor is NOT QUOTABLE" >&2; exit 1; }
./ci/floor-stamp-vocabulary-falsify.sh > /tmp/t0-floor-vocab-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/floor-stamp-vocabulary-falsify.sh: an injected fault went green" >&2
         cat /tmp/t0-floor-vocab-falsify.$$.log >&2
         rm -f /tmp/t0-floor-vocab-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-floor-vocab-falsify.$$.log
rm -f /tmp/t0-floor-vocab-falsify.$$.log

# --- entity reach ---------------------------------------------------------
# `ci/checks/entity-reach.toml` says which of the jar's 158 entity types the
# corpus puts in the world. Tree-only and about a second: it reads the check
# files and two jar-derived tables and never boots anything.
#
# The case it exists for is the third in the falsifier — a check that observes
# `add_entity` and is named in no row. That is not a typo class, it is what
# happens when the corpus grows an entity check and the inventory quietly stops
# being the inventory, and it is the same failure `ci/projection-drops.py`
# refuses one layer down for a dropped field.
if [ -e data/vanilla/entity_types.json ]; then
    python3 ci/entity-reach.py --check > /tmp/t0-entity-reach.$$.log 2>&1 \
        || { echo "T0 RED — ci/entity-reach.py: ci/checks/entity-reach.toml no longer describes the corpus" >&2
             cat /tmp/t0-entity-reach.$$.log >&2
             rm -f /tmp/t0-entity-reach.$$.log; exit 1; }
    tail -1 /tmp/t0-entity-reach.$$.log
    rm -f /tmp/t0-entity-reach.$$.log
    ./ci/entity-reach-falsify.sh > /tmp/t0-entity-reach-falsify.$$.log 2>&1 \
        || { echo "T0 RED — ci/entity-reach-falsify.sh: an injected fault went green" >&2
             cat /tmp/t0-entity-reach-falsify.$$.log >&2
             rm -f /tmp/t0-entity-reach-falsify.$$.log; exit 1; }
    tail -1 /tmp/t0-entity-reach-falsify.$$.log
    rm -f /tmp/t0-entity-reach-falsify.$$.log
elif [ "${RUSTMC_FIXTURES:-required}" = "required" ]; then
    # **A SKIP THAT STILL PRINTS `T0 GREEN` IS THE DEFECT THIS FILE'S OWN TAIL
    # BLOCK IS ABOUT.** `real_vanilla_chunks_survive_a_rewrite` skipped in every
    # gate run on the box for its whole life and printed `T0 GREEN` each time,
    # which is why `RUSTMC_FIXTURES` exists and why optional mode prints a
    # different string. A jar-derived table is a fixture by exactly that
    # definition, so its absence is a FAILURE in the default mode and a skip
    # only where every other fixture-backed check is also allowed to skip.
    echo "T0 RED — data/vanilla is absent, so ci/entity-reach.py could not run." >&2
    echo "         Generate it: ./ci/gen-gameplay-tables.sh (PATH must carry cargo)." >&2
    echo "         Or RUSTMC_FIXTURES=optional, which prints a different final line" >&2
    echo "         precisely so a gate cannot read this run as a result." >&2
    exit 1
else
    echo "entity-reach: SKIPPED, data/vanilla absent and RUSTMC_FIXTURES=${RUSTMC_FIXTURES}"
fi

# --- two shell idioms this project has got wrong more than once ---------------
#
# `pkill -f` matched its own caller and killed it five separate times in one
# run, three of them after it was documented. `cmd | grep -c X || echo 0` emits
# "0\n0" and was made twice. Being written down is what both classes have
# already had, and it is not what stops them.
#
# Comments and heredoc bodies are blanked before the search, and that is the
# point rather than tidiness: a raw grep returns twelve `pkill -f` in ci/ and
# four are comments WARNING against it. A checker counting those would report
# the documentation of a defect as the defect, and its ratchet would rise every
# time somebody recorded the lesson. About 0.2 s, no build.
python3 ci/shell-footguns.py \
    || { echo "T0 RED — an undeclared shell footgun, or a stale declaration" >&2; exit 1; }

python3 ci/shell-footguns-falsify.py > /tmp/t0-footguns-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/shell-footguns-falsify.py: an injected fault went green" >&2
         cat /tmp/t0-footguns-falsify.$$.log >&2
         rm -f /tmp/t0-footguns-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-footguns-falsify.$$.log
rm -f /tmp/t0-footguns-falsify.$$.log

# --- comments that claim a guard the code does not have -----------------------
#
# A comment is the one artefact here that nothing is run against, and it is the
# artefact a reader trusts most, because it is the only one that says what the
# code is FOR. `docs/PARITY-ROADMAP.md` §73.8 tabulated four in this repo and
# recorded as the finding that NONE was found by a check — all four were found
# by reading. Repairing them closed the instances and not the class.
#
# About 22 s, no build. The ratchet is `ci/checks/comment-guards.toml`: a
# declared instance that stops being found is as red as a new one.
python3 ci/check-comment-guards.py \
    || { echo "T0 RED — a comment claims a guard the code does not have" >&2; exit 1; }

./ci/comment-guards-falsify.sh > /tmp/t0-cguards-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/comment-guards-falsify.sh: a re-introduced original went green" >&2
         cat /tmp/t0-cguards-falsify.$$.log >&2
         rm -f /tmp/t0-cguards-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-cguards-falsify.$$.log
rm -f /tmp/t0-cguards-falsify.$$.log

# --- the findings index -------------------------------------------------------
#
# The same `figure-provenance` defect was found independently by FIVE lanes in
# the run of 2026-08-18/19, so four area-lanes went on a rediscovery, and one
# arithmetic error rotted in three places at once. `docs/FINDINGS.md` is the
# fix, and an index nobody appends to is worse than no index: it reads as
# evidence that nothing was found.
#
# So a branch that records a status change in `docs/` prose and does not index
# it is refused here. This is a TRIPWIRE and not a census — it fires on a
# vocabulary, it cannot judge whether the line appended describes the finding,
# and a lane recording a finding in words outside the vocabulary passes. What
# it removes is the one failure that actually happened: a retraction written
# into prose and walked away from, invisible to the next four lanes.
#
# It is a no-op in this tree. `ci/t0-remote.sh` rsyncs with `--exclude '.git'`,
# so there is no base to diff against and the script says NO-GIT out loud
# rather than printing green — the falsifier asserts that line. The path that
# has teeth is an agent's own worktree and `ci/handoff.sh`'s canonical
# checkout, which are real checkouts.
python3 ci/findings-index.py --check \
    || { echo "T0 RED — a finding was recorded in docs/ and not in docs/FINDINGS.md" >&2; exit 1; }

# Its own falsifier, for the reason every other falsifier runs here: a tripwire
# that fires on a vocabulary is the shape that goes quiet without anyone
# noticing, and a check whose red state is never demonstrated is this project's
# central defect class. Fifteen cases, about two seconds, no build.
./ci/findings-index-falsify.sh > /tmp/t0-findings-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/findings-index-falsify.sh: the findings tripwire stopped discriminating" >&2
         cat /tmp/t0-findings-falsify.$$.log >&2
         rm -f /tmp/t0-findings-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-findings-falsify.$$.log
rm -f /tmp/t0-findings-falsify.$$.log

# The brief gate is a PreToolUse hook, so nothing else in this repo runs it and
# nothing else notices when it stops deciding — which has already happened once,
# on 2026-08-17, found by an operator being blocked rather than by a check.
# Thirteen cases, about two seconds, no build.
./ci/brief-gate-falsify.sh > /tmp/t0-briefgate-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/brief-gate-falsify.sh: the agent brief gate stopped deciding" >&2
         cat /tmp/t0-briefgate-falsify.$$.log >&2
         rm -f /tmp/t0-briefgate-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-briefgate-falsify.$$.log
rm -f /tmp/t0-briefgate-falsify.$$.log

# The server has to build on Windows 10+, macOS, FreeBSD, Android (API 21)
# and Linux. `cargo check --locked` of the `rustmc-server` binary for each
# target, installing a missing `rust-std` with rustup; no C toolchain is
# involved because `check` stops before codegen. The falsifier injects a
# `std::os::unix` use into a copy of the workspace and requires Windows RED
# and the host GREEN, so a stage that stopped looking cannot stay green.
./ci/cross-check.sh > /tmp/t0-cross-check.$$.log 2>&1 \
    || { echo "T0 RED — ci/cross-check.sh: the server does not type-check for every platform it ships on" >&2
         cat /tmp/t0-cross-check.$$.log >&2; rm -f /tmp/t0-cross-check.$$.log; exit 1; }
tail -1 /tmp/t0-cross-check.$$.log; rm -f /tmp/t0-cross-check.$$.log
./ci/cross-check-falsify.sh > /tmp/t0-cross-check-falsify.$$.log 2>&1 \
    || { echo "T0 RED — ci/cross-check-falsify.sh: a Unix-only API in the server went unrefused" >&2
         cat /tmp/t0-cross-check-falsify.$$.log >&2; rm -f /tmp/t0-cross-check-falsify.$$.log; exit 1; }
tail -1 /tmp/t0-cross-check-falsify.$$.log; rm -f /tmp/t0-cross-check-falsify.$$.log

# MERGE-REGION END checks

# --- the sibling's version must not drift into Cargo.lock unreviewed --------
#
# The block above gates the sibling's CONTENT; this gates the one thing about
# it that a build can silently carry into `Cargo.lock`. `../plugins` is one
# shared, mutable checkout every worktree on the box points at (measured
# 2026-08-17: origin/main pins grit-plugin-api at 0.2.1, the sibling on disk
# was at 0.3.0, and nothing had bumped it on purpose). A path dependency still
# gets a `version` in the lock, cargo re-resolves it on every build, and the
# next branch that commits `Cargo.lock` for any reason carries that drift into
# main with no review and no mention. `ci/checks/dependency-pins.toml` and
# `ci/check-dependency-pins.py` — no build, no box, a few milliseconds of TOML
# reading, same argument as the figures/doc-claims checks above.
python3 ci/check-dependency-pins.py \
    || { echo "T0 RED — Cargo.lock has drifted from a declared dependency pin" >&2; exit 1; }

# The sentinel a gate greps for is only printed when the run could have failed.
#
# Vanilla-backed tests take a fixture from `oracle-work/`, and a missing fixture
# is a **failure** by default (`rustmc_data::fixture`) — so the line above is
# what makes a skip visible, and no grep is involved. `RUSTMC_FIXTURES=optional`
# turns those back into skips for a clean clone with no vanilla extract, and a
# run with skips in it is not a gate result.
#
# So optional mode prints a *different string*. `ci/handoff.sh` counts
# `^T0 GREEN`, gets 0, and reddens. Two different claims must not share a string
# a gate greps for — the rule `ci/t1-gate.sh` already applies to `T1 GREEN` vs
# `T1-GATE GREEN`, applied here to the claim "this run could have failed".
#
# Before this existed, `real_vanilla_chunks_survive_a_rewrite` skipped in every
# gate run on the box for its whole life and printed `T0 GREEN` each time.
# ci/t1.sh runs its test binaries through ci/t1-par.sh. That script's
# red-when-broken evidence is a throwaway cargo package run serially and in
# parallel: same result lines, same failure blocks, a signal-killed binary
# still red, cargo's env and cwd intact, and three mutants each rejected.
# 30.7 s wall and 7 s CPU inline (mostly its sleepers and one tiny compile
# in a temp target dir), so it is prefetched at the top and judged here.
t0_prefetched bash ci/t1-par-falsify.sh > /tmp/t0-t1par.$$.log 2>&1 \
    || { echo "T0 RED — ci/t1-par-falsify.sh: parallel T1 can lose a red or a test" >&2
         cat /tmp/t0-t1par.$$.log >&2; rm -f /tmp/t0-t1par.$$.log; exit 1; }
tail -1 /tmp/t0-t1par.$$.log
rm -f /tmp/t0-t1par.$$.log

# --- the skip ledger, adjudicated -----------------------------------------
#
# THE RULE THIS ENFORCES, stated so it can be checked rather than remembered:
#
#     A stage may skip only in an environment that could not have run it,
#     and the gate fails if it skips in one that could.
#
# `ci/box-skip-audit.py` reds on any skip `ci/checks/box-skips.toml` does not
# declare, which is the first half. `--require-ran` is the second: when this
# tree HAS an object store — which it does whenever `ci/t0-remote.sh` shipped
# it — the three repo-dependent checks are forbidden to stand down, so the
# store being present and the checks running are the same assertion rather than
# two hopes.
#
# Without that clause "declared" would drift into "permanently excused", and
# the rows in the declaration would quietly become three checks that run
# nowhere — which is the state this whole area was opened to repair.
if [ -s "$RUSTMC_BOX_SKIP_LOG" ] || [ -e .gitstore ] || [ -e .git ]; then
    # Each clause asks the SAME question the stage asked, and forbids the skip
    # when the answer is yes. A declaration says "this environment cannot run
    # it"; these lines are what stops that claim being taken on trust in an
    # environment where it is false.
    REQUIRE_RAN=""
    add_required() { REQUIRE_RAN="${REQUIRE_RAN:+$REQUIRE_RAN,}$1"; }
    if [ -e .gitstore ] || [ -e .git ]; then
        add_required "check-cited-audits"
        add_required "figure-provenance"
        add_required "figure-provenance:selftest-resolved"
        add_required "check-cited-audits-falsify:green-control"
    fi
    if [ -s oracle-work/corpus-floor/floor.json ]; then
        add_required "corpus-floor-falsify:replay"
    fi
    if [ -s oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar ]; then
        add_required "tick-section-classify:selftest"
    fi
    # `ci/lockfile-falsify.sh` stands down when the tree is not a repository, or
    # when `Cargo.lock` is already dirty — it will not touch a file somebody has
    # uncommitted work in. So it is required to have run exactly when neither
    # holds: a repository, with that file clean.
    if [ -e .git ] && [ -z "$(git status --porcelain -- Cargo.lock 2>/dev/null || true)" ]; then
        add_required "lockfile-falsify:tree-stays-clean"
    fi
    python3 ci/box-skip-audit.py --log "$RUSTMC_BOX_SKIP_LOG" \
            --require-ran "$REQUIRE_RAN" \
        || { echo "T0 RED — a stage stood down where it should have run, or a skip nobody declared" >&2
             rm -f "$RUSTMC_BOX_SKIP_LOG"; exit 1; }
fi
rm -f "$RUSTMC_BOX_SKIP_LOG"

# Keep-going reaches this line with reds recorded; it must not print GREEN.
# (Moved below stack-ci's skip-ledger audit on train-c, so that it stays
# immediately before the verdict as ci/t0-keepgoing-falsify.sh case F asks,
# and the audit's own red is counted before the guard reads the tally.)
t0_stages_verdict_guard
case "${RUSTMC_FIXTURES:-required}" in
    required) echo "T0 GREEN" ;;
    *) echo "T0 PASS — RUSTMC_FIXTURES=${RUSTMC_FIXTURES}, fixture-backed tests may have skipped; NOT a gate result" ;;
esac
