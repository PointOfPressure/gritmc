#!/usr/bin/env bash
# corpus-floor.sh — measure how much of the gameplay corpus vanilla cannot
# reproduce against itself, from recordings that already exist.
#
#     ./ci/corpus-floor.sh ~/rustmc*/oracle-work/gameplay/vanilla.json
#     ./ci/corpus-floor.sh --id nightly-6 --out /tmp/floor r1.json r2.json ...
#     ./ci/corpus-floor.sh --checks /tmp/control-corpus r1.json r2.json
#
# Reads `ci/corpus-floor.py`'s header before anything else here — the protocol
# is there, this file only assembles its input.
#
# # WHAT THIS COSTS, AND WHY THAT IS THE POINT
#
# Nothing but the recordings. `ci/t2-gameplay-selfcheck.sh` cold-boots one
# vanilla server per run at about twelve minutes each; this reads recordings
# already on the box and finishes in seconds, so a floor can be re-measured on
# every branch instead of once a fortnight by whoever had the time.
#
# The recordings it reads are the ordinary output of `ci/t2-gameplay.sh` —
# `oracle-work/gameplay/vanilla.json` in every tree that has run a corpus. Six
# of those existed on the Threadripper on 2026-08-09 from one night's work, and
# the whole first measurement was taken from them without booting anything.
#
# **It does not replace the selfcheck**, on the same terms
# `ci/selfcheck-recompare.sh` does not: every recording here was taken by
# whatever recorder the tree that took it had, so this says nothing about a
# change to the recorder or to the harness's interaction with a live server.
# What it does say is measured rather than assumed — the ensemble carries each
# run's recorder digest and `ci/corpus-floor.py` reports the homogeneous subset
# beside the full set.
#
# # IT REFUSES RATHER THAN REPORTING A WRONG FLOOR
#
# Fewer than two recordings, a recording that is not a trace, or a projection
# under the wrong lens: exit non-zero, every time. A floor of 0 from one
# recording would be this project's favourite kind of defect — an instrument
# that cannot fail — in the file that exists to catch them.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CHECKS="$ROOT/ci/checks/gameplay"
DECLARED="$ROOT/ci/checks/gameplay-unstable.toml"
FLOOR_FILE="$ROOT/ci/checks/gameplay-floor.toml"
OUT="${OUT:-$ROOT/oracle-work/corpus-floor}"
ID="$(date -u +%Y%m%dT%H%MZ)"
ADJUDICATE=1
UPDATE=0

RECORDINGS=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --checks)      CHECKS="$2"; shift 2 ;;
        --declared)    DECLARED="$2"; shift 2 ;;
        --out)         OUT="$2"; shift 2 ;;
        --id)          ID="$2"; shift 2 ;;
        --floor-file)  FLOOR_FILE="$2"; shift 2 ;;
        --no-adjudicate) ADJUDICATE=0; shift ;;
        # Rewrite the standing floor file from this measurement. Deliberately
        # not the default: the floor is a published figure and moving one is a
        # commit somebody signs, not a side effect of running a diagnostic.
        --update)      UPDATE=1; shift ;;
        -*)            echo "unknown argument: $1" >&2; exit 2 ;;
        *)             RECORDINGS+=("$1"); shift ;;
    esac
done

if [ "${#RECORDINGS[@]}" -lt 2 ]; then
    echo "corpus-floor: need at least 2 vanilla recordings, given ${#RECORDINGS[@]}" >&2
    echo "              usage: $0 [--id NAME] [--out DIR] <vanilla.json> <vanilla.json> ..." >&2
    exit 2
fi
[ -d "$CHECKS" ] || { echo "corpus-floor: no corpus at $CHECKS" >&2; exit 2; }

# Validate every input BEFORE building anything. Refusing after a two-minute
# release build is the same refusal delivered late.
for rec in "${RECORDINGS[@]}"; do
    [ -s "$rec" ] || { echo "corpus-floor: $rec is missing or empty" >&2; exit 2; }
    # **A recording, not a report.** `oracle-work/gameplay-selfcheck/` holds
    # `r1.json`...`rN.json` *and* `report-2.json`...`report-N.json`, so the
    # obvious `r*.json` glob picks up the aggregation's own output as input.
    # `ci/selfcheck-recompare.sh`'s header records this happening on its first
    # run — 26 recordings became "52" — and the fix there was a tighter glob,
    # which protects that one script and not its callers. This refuses the file
    # instead, so the trap is closed wherever the paths come from.
    python3 - "$rec" <<'PY' || exit 2
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception as e:
    print(f"corpus-floor: {sys.argv[1]} is not JSON: {e}", file=sys.stderr); raise SystemExit(1)
if "outcomes" in d and "scenarios" not in d:
    print(f"corpus-floor: {sys.argv[1]} is a comparison REPORT, not a recording. "
          f"The `r*.json` glob picks up `report-N.json`; use `r[0-9]*.json`.",
          file=sys.stderr)
    raise SystemExit(1)
if not isinstance(d.get("scenarios"), list) or "recorder" not in d:
    print(f"corpus-floor: {sys.argv[1]} is not an oracle-runner trace", file=sys.stderr)
    raise SystemExit(1)
PY
done

# --- two copies of one recording are one sample ------------------------------
#
# **This floor is the disagreement between independent runs of vanilla. A file
# copied to two paths is not two runs, and nothing above this line could tell.**
# Every recording is identified by where it was found; a byte-identical pair
# contributes a pairwise disagreement of exactly zero, and enough of them drive
# every scenario to `flaps == 0`, which this script reports as "the reference
# reproduced it". That is a floor of zero guaranteed by construction — an
# instrument measuring its own input, which is the defect class this repository
# gates on, arriving through the one door the ensemble's own sha256 column was
# already standing in front of without being read.
#
# It was theoretical until 2026-08-16. `ci/reference-cache.sh` now lets a gate
# serve a stored vanilla recording instead of taking a fresh one, and every tree
# that hits the cache leaves a byte-identical `oracle-work/gameplay/vanilla.json`
# behind. The glob in this script's own usage line —
# `./ci/corpus-floor.sh ~/rustmc*/oracle-work/gameplay/vanilla.json` — would then
# sweep up N copies of one draw and call them an ensemble. Measured that day: 55
# such files exist on this box and all 55 are byte-distinct, which is what the
# floor has always quietly depended on and never checked.
#
# So: refuse, name both paths, and say what to do. Deduplicating silently would
# be wrong — a caller who believes they passed six independent runs and actually
# passed two has a problem with their inputs, not with this script, and a
# quietly-reduced N would publish a resolving power nobody asked for.
DUPES="$(sha256sum "${RECORDINGS[@]}" | awk '{print $1}' | sort | uniq -d)"
if [ -n "$DUPES" ]; then
    echo >&2
    echo "corpus-floor: REFUSED — byte-identical recordings were passed as separate runs." >&2
    for d in $DUPES; do
        echo "  sha $d:" >&2
        sha256sum "${RECORDINGS[@]}" | awk -v s="$d" '$1==s {print "    " $2}' >&2
    done
    echo "  A copy of a recording is not another sample of vanilla. Two paths holding" >&2
    echo "  one file disagree by zero at every scenario, and enough of that reports" >&2
    echo "  every scenario as reproduced — a floor of zero this script manufactured." >&2
    echo "  If these came from RUSTMC_REFERENCE_CACHE, they are one recording served" >&2
    echo "  N times: pass it once, and get the other samples from runs that recorded" >&2
    echo "  their own (ci/t2-gameplay-selfcheck.sh, or a gate with the cache off)." >&2
    exit 2
fi

cargo build -q -p oracle-runner --release
BIN="$ROOT/target/release/oracle-runner"

# **The identity of the comparator this floor is being taken under.** Read out
# of the binary that is about to do the projecting, not out of the tree it came
# from — see `compare::PROJECTION_SOURCES`. A floor is a number under a
# comparator and without this it does not say which one; `gameplay-container-throw`
# changed `compare::project` on 2026-08-09 and the standing floor of 9/88 went
# on being printed beside a corpus score, internally consistent and describing
# a projection that no longer existed.
PROJECTION="$("$BIN" projection-id)" || {
    echo "corpus-floor: the binary cannot state its projection identity" >&2; exit 2; }
case "$PROJECTION" in
    proj:????????????????) ;;
    *) echo "corpus-floor: projection-id returned '$PROJECTION', not proj:<16 hex>" >&2; exit 2 ;;
esac
echo "corpus-floor: projection $PROJECTION"

rm -rf "$OUT"; mkdir -p "$OUT"
MANIFEST="$OUT/ensemble.json"
: > "$OUT/runs.tsv"

i=0
for rec in "${RECORDINGS[@]}"; do
    i=$((i + 1))
    id="r$i"
    "$BIN" project "$CHECKS" --trace "$rec" --json "$OUT/$id-declared.json"
    "$BIN" project "$CHECKS" --trace "$rec" --json "$OUT/$id-blind.json" \
        --lens boundary-blind
    # **What this recording's DECODER could name**, as distinct from what the
    # projection does with it. `PROJECTION` above says which comparator; this
    # says what there was to compare. The two are independent and §15.4 is the
    # case that proves it: `compare::exempt_item_spawn_yaw` acts on a `y_rot`
    # the `twin-homog-20260809` recordings do not carry, so it is inert over
    # that ensemble and live over any run taken since — one projection id,
    # two instruments. `ci/trace_vocabulary.py` holds the whole rule; it is
    # imported rather than reimplemented by both readers, because a second
    # implementation of a comparison is a second answer waiting for an occasion.
    python3 "$ROOT/ci/trace_vocabulary.py" "$rec" > "$OUT/$id-vocab.json" || {
        echo "corpus-floor: cannot take a vocabulary from $rec" >&2; exit 2; }
    sha="$(sha256sum "$rec" | cut -d' ' -f1)"
    # The tree that took the recording, from the `.prov` file `ci/t2-gameplay.sh`
    # writes beside it, and the digest of that tree's recorder sources. Two
    # recordings taken by different decoders are not two samples of one
    # quantity, and this is what lets `ci/corpus-floor.py` say so rather than
    # leaving a reader to notice.
    tree=""
    [ -s "$rec.prov" ] && tree="$(sed -n 's/^tree=//p' "$rec.prov" | head -1)"
    # `ci/t2-gameplay-selfcheck.sh` writes `oracle-work/gameplay-selfcheck/rN.json`
    # with no `.prov` beside it — those recordings never leave the tree that
    # made them, so nothing had to stamp them. They are also the *best* input
    # this script can have, N cold boots of one recorder, and without this line
    # every one of them would come back `recorder_digest = unknown`.
    # **The fallback walks up from the RECORDING, not down from `$ROOT`.**
    #
    # It was `case "$(readlink -f "$rec")/" in "$ROOT"/*) tree="$ROOT";; esac`,
    # which answers "is this recording inside the tree doing the measuring".
    # That is a question about who is holding the ruler, and the recorder is a
    # property of the recording. Measured 2026-08-09: the six
    # `twin-homog-20260809` recordings live in `~/rustmc-twin` and were
    # measured from `~/rustmc-twin` when the floor was first taken, so all six
    # resolved to one recorder and the run reported `recorders = 1`. Re-running
    # the *identical* six from `~/rustmc-unstable` resolved none of them, gave
    # each `unknown-rN`, and the report came back **"6 distinct recorders,
    # largest homogeneous subset is 1, NO HOMOGENEITY CONTROL"** — the same
    # recordings, the same answer, and a corroboration column that had silently
    # emptied. A floor is supposed to be re-measurable from any tree; this made
    # it re-measurable only from the tree that took it, and the difference was
    # visible only to somebody who already knew what the answer should be.
    if [ -z "$tree" ]; then
        d="$(dirname "$(readlink -f "$rec")")"
        while [ "$d" != "/" ]; do
            [ -d "$d/tools/oracle-runner/src" ] && { tree="$d"; break; }
            d="$(dirname "$d")"
        done
    fi
    # **`unknown` must not be a group.** Grouping by a literal `unknown` would
    # put every unstamped recording in one homogeneous subset and report a
    # heterogeneity control that had checked nothing — which is worse than no
    # control, because the run prints `1 distinct recorder` and means `I could
    # not tell`. So an unidentifiable recorder is unique to its own run and the
    # subset it forms is of size one, which the report then says out loud.
    # **The stamp first, the tree only as a fallback, and the fallback is
    # labelled.** A `recorder=` line in the `.prov` was written when the
    # recording was made and is the truth. Hashing the tree's sources instead
    # answers "what recorder is in that tree NOW", which is a different
    # question with the same shape: two recordings taken from one tree at two
    # different commits hash the same and are grouped as one recorder, which is
    # exactly the confound the grouping exists to catch.
    #
    # So the fallback is prefixed `tree-now:`, and it says in the report which
    # runs carried a real stamp. **Both harnesses write one as of 2026-08-09**:
    # `ci/t2-gameplay-selfcheck.sh` and `ci/t2-gameplay.sh` alike run
    # `oracle-runner recorder-id`, which digests the embedded text of
    # `compare::RECORDER_SOURCES` and so describes the binary that recorded
    # rather than the tree it stood in. `docs/PARITY-ROADMAP.md` §13 named the
    # second of those as an errand and §15.4 as the weaker half of the
    # vocabulary question; it is done.
    #
    # Recordings taken before that carry either no stamp (the `tree-now:`
    # fallback below) or the selfcheck's older bare-hex form, and both group
    # apart from a `rec:` one. That is over-splitting, and over-splitting only
    # ever shrinks the homogeneous subset — the safe direction, since a smaller
    # subset can make a flap *suspect* and never *admissible*.
    rdig="unknown-$id"
    stamped=""
    [ -s "$rec.prov" ] && stamped="$(sed -n 's/^recorder=//p' "$rec.prov" | head -1)"
    if [ -n "$stamped" ]; then
        rdig="$stamped"
    elif [ -n "$tree" ] && [ -d "$tree/tools/oracle-runner/src" ]; then
        rdig="tree-now:$(cat "$tree"/tools/oracle-runner/src/*.rs | sha256sum | cut -c1-16)"
    fi
    when="$(date -u -r "$rec" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo unknown)"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$id" "$rec" "$sha" "${tree:-unknown}" "$rdig" "$when" >> "$OUT/runs.tsv"
done


# The tsv is passed by path and not on stdin: a heredoc IS stdin, so
# `python3 - ... < runs.tsv <<'PY'` silently reads the script and an empty
# table, and the manifest comes out with zero runs. It did, on the first run of
# this script, and `ci/corpus-floor.py` refused it — which is the refusal
# working, one step later than it should have had to.
python3 - "$MANIFEST" "$ID" "$OUT" "$OUT/runs.tsv" "$PROJECTION" <<'PY'
import json, sys
manifest, ident, out, tsv, projection = sys.argv[1:6]
runs = []
for line in open(tsv):
    rid, src, sha, tree, rdig, when = line.rstrip("\n").split("\t")
    runs.append({
        "id": rid, "source": src, "sha256": sha, "tree": tree,
        "recorder_digest": rdig, "recorded": when,
        "declared": f"{out}/{rid}-declared.json",
        "blind": f"{out}/{rid}-blind.json",
        "vocabulary": f"{out}/{rid}-vocab.json",
    })
json.dump({"id": ident, "projection": projection, "runs": runs},
          open(manifest, "w"), indent=1)
print(f"corpus-floor: ensemble {ident}, {len(runs)} recording(s) -> {manifest}")
PY

# --- rule 7: two instruments that can disagree are two instruments ----------
#
# The floor is derived from `oracle-runner project`, and every gate figure in
# this tree is derived from `oracle-runner compare`. They share `compare::project`
# so they cannot drift in principle — but "cannot drift in principle" is what
# was said about the `data/vanilla` symlinks, and this project's rule is that a
# second instrument which can disagree with the first is a defect waiting for
# an occasion.
#
# So one pair is done both ways on every run and the two answers must be
# identical over the scenarios both recordings contain. It costs one `compare`.
#
# **The intersection is not a fudge and is the same one `ci/selfcheck-recompare.sh`
# makes.** A scenario a recording predates is missing from one side and
# `compare` reports `error` for it, which is a statement about the recording's
# age and not about the reference. Measured on r1/r2 of the first real ensemble:
# `compare` reported 10 non-match, the projection reported 8, and the two extras
# were exactly `a-player-says-something` and `server-answers-list-uuids`, the two
# scenarios those recordings predate. Over the 86 in common the two sets were
# equal.
#
# # "NOT MATCH" IS NOT "THE PROJECTIONS DIFFER", AND THE TEST USED TO ASSUME IT
#
# Corrected 2026-08-11, on the branch that rebased the container state id. The
# comparison below used to be `proj != cmpd` with
# `cmpd = {verdict != "match"}`, and that is a category error with a refusal on
# the end of it: **three of `compare`'s verdicts are relabels of `Match`.**
# `Inert`, `Mute` and `Vacuous` are each guarded on
# `out.verdict == Verdict::Match` in `compare.rs`, so every one of them asserts
# the two declared projections are byte-EQUAL while not being the string
# `match`. Demanding that such a scenario also appear in `proj` demands a
# contradiction.
#
# It could not fire while every scenario of that class also happened to differ
# for some other reason. The state-id rebase removed the other reason:
# `container-click-pickup-all-gathers-matching-stacks` went `diverge` -> `match`
# -> relabelled `mute`, landed in `cmpd` and not in `proj`, and this script
# refused a correct measurement. The instrument was wrong, not the input.
#
# So the two sets are now built from the same claim rather than from its
# complement. `diverge` and `absent` are the verdicts that assert a difference;
# the four that assert none are not required to appear in `proj`, and a verdict
# this script does not classify is a refusal rather than a silent pass — which
# is the direction `Verdict::baselineable` fails in for the same reason.
#
# Note this is STRICTER than what it replaces, not laxer: a `mute` or `inert`
# scenario whose projections genuinely differ is now caught, and before it was
# indistinguishable from the contradiction above.
"$BIN" compare "$CHECKS" --reference "${RECORDINGS[0]}" --subject "${RECORDINGS[1]}" \
    --json "$OUT/cross-check.json" > "$OUT/cross-check.log" 2>&1 || true
python3 - "$OUT/cross-check.json" "$OUT/r1-declared.json" "$OUT/r2-declared.json" <<'PY' \
    || { echo "corpus-floor: REFUSED — the two instruments disagree" >&2; exit 3; }
import json, sys

# The verdicts that assert the two declared projections differ, and the ones
# that assert they do not. `error` is in neither: a side that would not run
# says nothing either way. Kept as two explicit sets so that a verdict added
# later lands in neither and is refused, rather than being absorbed by a
# `!=` against one of them.
DIFFERS = {"diverge", "absent"}
SAME = {"match", "mute", "inert", "vacuous"}

rep, a, b = (json.load(open(p)) for p in sys.argv[1:4])
da = {s["name"]: s for s in a["scenarios"]}
db = {s["name"]: s for s in b["scenarios"]}
common = {n for n in da if da[n]["present"] and db.get(n, {}).get("present")}
key = lambda s: json.dumps(s["declared"], sort_keys=True)
proj = {n for n in common if key(da[n]) != key(db[n])}
verdict = {o["name"]: o["verdict"] for o in rep["outcomes"]}

cmpd = {n for n in common if verdict.get(n) in DIFFERS}
same = {n for n in common if verdict.get(n) in SAME}
unclassified = sorted(
    (n, verdict.get(n, "<missing>"))
    for n in common
    if verdict.get(n) not in DIFFERS | SAME | {"error"}
)
# `compare` found a difference where the projections are identical.
bad_diff = sorted(cmpd - proj)
# The projections differ and `compare` called it one of the verdicts that
# claims they do not. This is the half the old test could not express.
bad_same = sorted(proj & same)
if bad_diff or bad_same or unclassified:
    print(f"  compare says differ, projection says equal: {bad_diff}")
    print(f"  projection says differ, compare says equal: {bad_same}")
    print(f"  verdicts this cross-check cannot classify:  {unclassified}")
    raise SystemExit(1)
print(
    f"corpus-floor: cross-check OK — `compare` and `project` agree on all "
    f"{len(common)} scenarios common to r1 and r2 "
    f"({len(proj)} differ, {len(same)} agree)"
)
PY

"$ROOT/ci/corpus-floor.py" measure "$MANIFEST" --out "$OUT/floor.json"
if [ "$UPDATE" = 1 ]; then
    "$ROOT/ci/corpus-floor.py" update "$OUT/floor.json" --out "$FLOOR_FILE"
fi
if [ "$ADJUDICATE" = 1 ]; then
    "$ROOT/ci/corpus-floor.py" adjudicate "$OUT/floor.json" \
        --declared "$DECLARED" --floor-file "$FLOOR_FILE"
fi
