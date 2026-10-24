#!/usr/bin/env bash
# be-emit-falsify.sh — break `block_entity.rs` on purpose, one bug at a time,
# and report every bug its tests did NOT catch.
#
#     ./ci/be-emit-falsify.sh                 # all injections
#     ./ci/be-emit-falsify.sh <name>          # just one
#
# WHY THIS EXISTS. `crates/rustmc-worldgen/src/block_entity.rs` shipped with
# six unit tests and all six were green on the first run. `docs/AGENT-BRIEF.md`
# rule 7: when a measurement clears every candidate, suspect the measurement.
# This project's central defect is a check whose green cannot be turned red,
# and a test suite written by the same person in the same hour as the code is
# the most likely place in this branch for one to be hiding. So the assumption
# here is that a hole exists and the job is to find it.
#
# It is also the specific bar the HARD RULE at the top of `docs/AGENT-BRIEF.md`
# sets: anything added ships with recorded red-when-broken evidence.
# `ci/corpus-falsifiable.py` and `ci/liveness-proofs.py` mechanise that for
# gameplay scenarios and readiness proofs; this is neither, so it carries its
# own.
#
# WHAT IT COSTS. **41 seconds** for the whole sweep, locally, measured on the
# Threadripper 2026-08-16 — about 4 s per injection. That is cheap enough to
# gate, and it is in `ci/t0.sh`.
#
# The first estimate written here was `about three and a half minutes`,
# extrapolated from a single timed `cargo test` that included a cold rebuild.
# It was wrong by 5x because an incremental debug rebuild of one crate is
# nothing like the first one, and it is corrected rather than deleted because
# a cost written down from one sample is how a cheap check gets left out of a
# gate for years. The next person adding an injection is adding ~4 s. Unlike `ci/collide-falsify.sh` this needs no
# remote directory and no rsync: the whole subject is one file and a `cargo
# test --lib block_entity`, and serialising it behind a remote dir would buy
# an isolation nothing here needs. It takes no lock and touches no reference.
#
# Each injection is a transcription error a careful reader could plausibly
# make, applied to the real source, tested, and reverted — `trap` restores the
# file even on a kill, because a falsify script that leaves poisoned source
# behind is worse than no falsify script.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

SRC="crates/rustmc-worldgen/src/block_entity.rs"
ONLY="${1:-}"
BACKUP="$(mktemp)"
cp "$SRC" "$BACKUP"
restore() { cp "$BACKUP" "$SRC"; rm -f "$BACKUP"; }
trap restore EXIT INT TERM

# name @ old @ new @ expect
#
# THE SEPARATOR IS `@` AND IT USED TO BE A TAB. That was a real bug in this
# script, found by this script's own first run and kept here because it is the
# exact defect class the file exists to hunt. Tab is IFS *whitespace*, so
# `read` collapses a run of them: the two injections with an empty `new`
# column — the ones that DELETE a line, which are the sharpest tests there are
# — had `new` read as "caught" and `expect` read as empty. They applied a
# nonsense patch, the suite went red for the wrong reason, and `expect:result`
# matched no `case` branch, so they printed nothing at all. Eight results were
# reported for ten injections and the summary line said `injections=10
# holes=0`. **A sweep that loses a case silently is a sweep that fails open**,
# which is worse than the hole it was looking for. The `unknown expect` guard
# below is the mechanised version of the fix; the separator change alone would
# have left the next empty column to rediscover it.
#
# `expect` is the point of the fourth column and it is not documentation:
#
#   caught         the tests must go red. If they do not, the suite has a hole.
#   unobservable   the injected change **cannot** alter the emitted NBT, for a
#                  reason written down below. If the tests go RED on one of
#                  these, the reason is wrong, and that is as much a finding as
#                  a hole. Either direction fails this script.
#
# The `unobservable` argument, which this script will falsify if it is wrong:
#
#   insert_order    the compound is a `BTreeMap`, so the order the keys are
#     inserted in cannot reach the serialised bytes — iteration is by key.
#     This is the control: it proves the harness can tell "the tests noticed"
#     from "the build changed at all", and without it a script where every
#     injection is caught cannot distinguish a good suite from a suite that
#     goes red on any edit whatsoever.
INJECTIONS=$(cat <<'EOF'
countdown_value@Nbt::Int(-1)@Nbt::Int(0)@caught
keep_packed_true@m.insert("keepPacked".into(), Nbt::Byte(0));@m.insert("keepPacked".into(), Nbt::Byte(1));@caught
components_dropped@m.insert("components".into(), Nbt::Compound(BTreeMap::new()));@@caught
id_key_dropped@m.insert("id".into(), Nbt::String(name.into()));@@caught
coords_x_z_swapped@chunk_x * 16 + (i % 16) as i32,@chunk_z * 16 + (i % 16) as i32,@caught
cell_y_stride@base_y + (i / 256) as i32,@base_y + (i / 16) as i32,@caught
section_filter_inverted@if !section@if section@caught
name_not_stripped@let name = state.split('[').next().unwrap_or(state);@let name = state.as_str();@caught
reproduce_everything@pub fn is_reproduced(name: &str) -> bool {\n    matches!(@pub fn is_reproduced(name: &str) -> bool {\n    !name.is_empty() || matches!(@caught
insert_order@m.insert("components".into(), Nbt::Compound(BTreeMap::new()));\n    m.insert("id".into(), Nbt::String(name.into()));@m.insert("id".into(), Nbt::String(name.into()));\n    m.insert("components".into(), Nbt::Compound(BTreeMap::new()));@unobservable
EOF
)

# Debug, not release. The sweep's whole cost is recompiling `rustmc-worldgen`
# eleven times, and the tests here are six assertions over a `BTreeMap` — there
# is nothing for the optimiser to make faster and a release rebuild of that
# crate is several times dearer. Measured: ~4 s per injection, 41 s for the
# sweep.
run_tests() {
  cargo test -p rustmc-worldgen --lib block_entity >/dev/null 2>&1
}

echo "=== baseline: the suite must be GREEN before anything is injected ==="
if ! run_tests; then
  echo "BASELINE RED — the suite fails on unmodified source. Nothing below means anything." >&2
  exit 2
fi
echo "baseline GREEN"
echo

holes=0
surprises=0
n=0
while IFS='@' read -r name old new expect; do
  [ -z "${name:-}" ] && continue
  [ -n "$ONLY" ] && [ "$ONLY" != "$name" ] && continue
  n=$((n + 1))
  cp "$BACKUP" "$SRC"
  # python rather than sed: the patterns contain regex metacharacters and
  # newlines, and a sed that silently matched nothing would report every
  # injection as an uncaught hole — a falsify script that fails open.
  if ! python3 - "$SRC" "$old" "$new" <<'PY'
import sys, pathlib
path, old, new = sys.argv[1], sys.argv[2].replace("\\n", "\n"), sys.argv[3].replace("\\n", "\n")
t = pathlib.Path(path).read_text()
if t.count(old) < 1:
    sys.exit(3)
pathlib.Path(path).write_text(t.replace(old, new, 1))
PY
  then
    echo "STALE   $name — its pattern no longer matches $SRC. An injection that"
    echo "        applies nothing is a hole in this sweep, not a passing case."
    holes=$((holes + 1))
    continue
  fi

  if run_tests; then result=green; else result=red; fi

  case "$expect:$result" in
    caught:red)        echo "ok      $name — caught" ;;
    caught:green)      echo "HOLE    $name — injected and the suite stayed GREEN"; holes=$((holes + 1)) ;;
    unobservable:green) echo "ok      $name — unobservable, and the suite agreed" ;;
    unobservable:red)  echo "SURPRISE $name — argued unobservable, suite went RED. The argument is wrong."; surprises=$((surprises + 1)) ;;
    *)
      # The hole this script found in ITSELF. A row whose `expect` is neither
      # value fell through every branch and printed nothing, so a lost case
      # was invisible in a summary that still said holes=0. Never again
      # silently: an unreadable row is a broken sweep, not a passing case.
      echo "MALFORMED $name — expect='$expect' is neither 'caught' nor 'unobservable'."
      echo "          The row did not parse. A sweep that cannot read its own"
      echo "          table cannot be trusted to have run the cases in it."
      holes=$((holes + 1)) ;;
  esac
done <<< "$INJECTIONS"

restore
trap - EXIT INT TERM
echo
echo "BE-EMIT-FALSIFY injections=$n holes=$holes surprises=$surprises"
[ "$holes" -eq 0 ] && [ "$surprises" -eq 0 ] || exit 1
