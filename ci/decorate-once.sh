#!/usr/bin/env bash
# decorate-once.sh — run DecorateGolden ONCE at an aimed centre, and KEEP the
# draw log.
#
# `ci/decorate-golden.sh` is the gate's generator: it runs the dumper twice,
# demands the two runs agree, and writes the committed vectors. It also runs in
# a `mktemp -d` that its own trap deletes — and `-Ddecorate.draws=<substring>`
# writes its `DRAW <n> <method>(<bound>) -> <value>` trace to **stderr**, which
# lands in `$OUT.log` inside that temp dir. So the one output you actually want
# when chasing a divergence is deleted on exit, and asking for it again means
# paying for two more full runs.
#
# That is what this is for: one run, aimed, log kept beside the JSON. It is not
# a gate and must never write `crates/rustmc-worldgen/golden/decorate_vectors.json`
# — regenerating the committed vectors is `ci/decorate-golden.sh`'s job,
# because only it proves the two runs agree.
#
#   ci/decorate-once.sh <out.json> [-D... ...]
#
# Typical use — the whole reason this exists:
#
#   ci/decorate-once.sh /tmp/mr.json \
#       -Ddecorate.seed=31415926 -Ddecorate.centers=65,63 \
#       -Ddecorate.draws=monster_room_deep
#   grep DRAW /tmp/mr.json.log
#
# **Check for a `DRAWS-END` after every `DRAWS-BEGIN` before you believe the
# log.** The dumper streams the trace to stderr as it goes, so a run that dies
# part way through leaves a complete-looking log of everything before the
# failure and no sign at all of what it never reached. That is not
# hypothetical: `GenLevel` threw on two `WorldGenLevel` no-ops `SculkSpreader`
# calls, and a five-centre `trees_jungle` run reported three chunks and read as
# "only three chunks here have jungle" — it had reported fifteen once the
# harness could finish. One line does it:
#
#   diff <(grep -c DRAWS-BEGIN /tmp/mr.json.log) \
#        <(grep -c DRAWS-END   /tmp/mr.json.log)
#
# Grep the log **unanchored**: `Bootstrap.bootStrap()` redirects the streams
# through log4j, which re-emits each line with a
# `[HH:MM:SS] [main/INFO]: [STDERR]: ` prefix, so `grep -E '^DRAW'` matches
# nothing and exits 1 — indistinguishable from a clean run that found nothing.
#
# `-Ddecorate.region=1` is set before the caller's arguments, so it is the
# default and a later `-D` on the command line still wins. Without it the
# dumper decorates only the middle chunk of a fresh 3x3 — a world no server
# produces. See the long note in `ci/decorate-golden.sh`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:?usage: ci/decorate-once.sh <out.json> [-D... ...]}"
shift

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

case "$OUT" in
    *golden/decorate_vectors.json)
        echo "refusing to write the committed vectors — use ci/decorate-golden.sh" >&2
        exit 2
        ;;
esac

# Same signature stripping as the gate's generator, for the same reason: the
# dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed jar.
UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
# A stable class cache rather than a temp dir, so aiming the dumper twice in a
# row does not pay javac twice. Rebuilt whenever a source is newer.
WORK="${RUSTMC_DECORATE_CLASSES:-$ROOT/oracle-work/goldengen-classes/decorate-once}"
mkdir -p "$WORK"
SRC=(
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java"
    "$ROOT/tools/goldengen/vanilla/GenLevel.java"
    "$ROOT/tools/goldengen/vanilla/DecorateGolden.java"
)
STAMP="$WORK/.built"
if [ ! -f "$STAMP" ] || [ -n "$(find "${SRC[@]}" -newer "$STAMP" -print -quit)" ]; then
    javac -nowarn -cp "$CP" -d "$WORK" "${SRC[@]}" 2>&1 | grep -v '^Note' || true
    touch "$STAMP"
fi

# stderr kept, not discarded — it is the entire point of this script. `|| true`
# because `set -e` would otherwise kill the script at this line when the JVM
# exits non-zero, before the diagnostic below could print the exception.
java -Drustmc.data="$VDIR/generated-data" -Ddecorate.out="$OUT" \
    -Ddecorate.region=1 "$@" -cp "$CP:$WORK" \
    net.minecraft.world.level.levelgen.DecorateGolden > "$OUT.log" 2>&1 || true

if [ ! -s "$OUT" ]; then
    echo "DecorateGolden wrote nothing; its stderr:" >&2
    tail -40 "$OUT.log" >&2
    exit 1
fi

# THE MISSING JSON IS NOT THE ONLY WAY THIS RUN FAILS, AND IT IS NOT THE ONE
# THAT COSTS ANYTHING.
#
# `-Ddecorate.draws` streams its trace to stderr as the run goes, so a run that
# dies part way through leaves a complete-looking log of every chunk before the
# failure and no trace at all of the ones it never reached. The exit above
# catches it only because no JSON was written; a reader who already has the log
# open sees nothing wrong with it. On 2026-08-18 a five-centre `trees_jungle`
# run reported three chunks that way and was read as "only three chunks here
# have jungle" — it reports fifteen once the harness can finish.
#
# So the dumper closes every run with `DRAWS-END`, and this pairs them. A
# `DRAWS-BEGIN` without its `DRAWS-END` names the chunk the run died in, which
# is the one piece of information the stack trace does not carry.
#
# Shown refusing before it was shown passing: `ci/decorate-once-falsify.sh`,
# six cases, two of them refusals.
if [ -n "${DRAWS_ARG:=$(printf '%s\n' "$@" | grep -m1 -- '-Ddecorate.draws=' || true)}" ]; then
    b=$(grep -c 'DRAWS-BEGIN ' "$OUT.log" || true)
    e=$(grep -c 'DRAWS-END ' "$OUT.log" || true)
    if [ "$b" != "$e" ]; then
        echo "DRAW TRACE TRUNCATED: $b DRAWS-BEGIN and $e DRAWS-END in $OUT.log." >&2
        echo "The run died inside the chunk named by the last unpaired BEGIN:" >&2
        # Every BEGIN whose chunk never got an END. Keyed on the chunk rather
        # than on order, because the dumper interleaves features per chunk.
        grep -oE 'DRAWS-(BEGIN|END) [^ ]+ chunk [-0-9,]+' "$OUT.log" \
            | awk '{k=$2" "$4} $1=="DRAWS-BEGIN"{b[k]++} $1=="DRAWS-END"{e[k]++} \
                   END{for (k in b) if (b[k]!=e[k]) print "  unpaired:", k}' >&2
        echo "Its exception, if it printed one:" >&2
        grep -m1 -A 4 'Exception in thread' "$OUT.log" >&2 || true
        exit 3
    fi
fi

echo "vectors -> $OUT"
echo "draw log -> $OUT.log   (grep unanchored: grep 'DRAW ' $OUT.log)"
