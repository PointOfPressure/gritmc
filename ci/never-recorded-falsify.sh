#!/usr/bin/env bash
# Prove `ci/never-recorded.py` refuses each defect it claims to catch.
#
# `docs/AGENT-BRIEF.md`: a guard that cannot be shown catching a real instance
# is itself a check that cannot fail. The verdict this ledger exists to protect
# is `vacant` — "the pinned jar contains no code that builds this packet, so
# refusing the id costs nothing" — and that is a claim about a 24 MB binary
# somebody will one day replace. So the central case here does not mutate the
# ledger to make the claim false; it **mutates the jar**, which is the way the
# claim will actually stop being true.
#
# Copies rather than edits: mutations go into `$(mktemp -d)` and the checker is
# pointed at them with `NEVER_RECORDED_ROOT` and `NEVER_RECORDED_JAR`. Nothing
# here writes to the working tree, and it needs no server and no recordings.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2
ROOT=$PWD
JAR=${NEVER_RECORDED_JAR:-$ROOT/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar}
PASS=0
FAIL=0

if [ ! -f "$JAR" ]; then
    echo "never-recorded-falsify: no pinned jar at $JAR" >&2
    echo "  every case here reads it; a SKIP would be a suite that tested nothing" >&2
    exit 2
fi

# `want <expect-exit> <name> <mutation-fn>`
# The mutation rewrites `$dir` in place, and may print a replacement jar path
# on stdout; printing nothing means "use the pinned jar".
want() {
    local expect=$1 name=$2 fn=$3
    local dir out jarfile
    dir=$(mktemp -d) || exit 2
    mkdir -p "$dir/ci/checks" "$dir/tools/oracle-runner/src"
    # **The fixture, not the ledger, and the drops put back.** Since
    # 2026-09-24 the real tree drops nothing at record time and the real
    # ledger has no rows, so every jar-side case below would have nothing to
    # act on. The scratch tree is therefore the pre-`recorder-housekeeping`
    # state in today's syntax: the twelve-row ledger as it stood
    # (`never-recorded.fixture.toml`), and each of its kinds dropped by an
    # early `return None` in `observe` — the only shape a record-time drop can
    # take now, and the one `ci/trace_source.py` reads.
    cp "$ROOT/ci/checks/never-recorded.fixture.toml" "$dir/ci/checks/never-recorded.toml"
    cp "$ROOT/tools/oracle-runner/src/trace.rs" "$dir/tools/oracle-runner/src/"
    python3 - "$dir" <<'PYEOF' || exit 2
import re, sys, tomllib
d = sys.argv[1]
names = [r["name"] for r in tomllib.load(open(d + "/ci/checks/never-recorded.toml", "rb"))["kind"]]
p = d + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
a = ") -> Option<(String, BTreeMap<String, serde_json::Value>)> {\n    if let Some((kind, mut f, consumed)) = decode("
assert s.count(a) == 1, "anchor moved — the fixture drops cannot be put back"
head, rest = a.split("\n", 1)
cond = "".join(f"\n        || id == cb::{n}" for n in names)
open(p, "w", encoding="utf-8").write(
    s.replace(a, head + "\n    if false" + cond + "\n    {\n        return None;\n    }\n" + rest, 1)
)
PYEOF
    jarfile=$("$fn" "$dir")
    [ -n "$jarfile" ] || jarfile=$JAR
    out=$(mktemp)
    NEVER_RECORDED_ROOT="$dir" NEVER_RECORDED_JAR="$jarfile" \
        python3 "$ROOT/ci/never-recorded.py" >"$out" 2>&1
    local got=$?
    if [ "$got" = "$expect" ]; then
        PASS=$((PASS + 1))
        printf 'ok    %-32s exit %s\n' "$name" "$got"
    else
        FAIL=$((FAIL + 1))
        printf 'FAIL  %-32s exit %s, wanted %s\n' "$name" "$got" "$expect"
        sed 's/^/        /' "$out"
    fi
    rm -rf "$dir" "$out"
    [ "$jarfile" = "$JAR" ] || rm -f "$jarfile"
}

control() { :; }

# --- the real one -----------------------------------------------------------
#
# A `vacant` row is a statement about the reference implementation, and the way
# it stops being true is a version bump in which something starts sending the
# packet. Reconstructed literally: the pinned jar with one extra class added,
# whose constant pool names `ClientboundPingPacket`. Under this guard, a jar in
# which vanilla began sending pings does not pass as "costs nothing".
#
# The added entry is a stub outside the protocol package whose bytes contain the
# Ping class's internal name. The scan is a constant-pool containment test by
# design (see the module docs), so that is exactly the signal a genuine new
# sender produces — and compiling a real sender inside a falsifier would be a
# second implementation of the thing under test.
vacant_gains_a_sender() {
    local jar
    jar=$(mktemp --suffix=.jar) || exit 2
    cp "$JAR" "$jar"
    python3 - "$jar" <<'PYEOF'
import sys
import zipfile

jar = sys.argv[1]
with zipfile.ZipFile(jar, "a") as z:
    z.writestr(
        "net/minecraft/server/network/FalsifierPingSender.class",
        b"\xca\xfe\xba\xbe" + b"net/minecraft/network/protocol/common/ClientboundPingPacket",
    )
PYEOF
    echo "$jar"
}

# A kind added to `is_housekeeping` and ranked nowhere: a packet removed from
# every recording that nobody said anything about.
unranked_kind() {
    sed -i 's/        || id == cb::UPDATE_ADVANCEMENTS/        || id == cb::UPDATE_ADVANCEMENTS\n        || id == cb::SET_HEALTH/' \
        "$1/tools/oracle-runner/src/trace.rs"
}

# The reverse: the recorder starts accepting a kind and the row stays, so the
# ledger describes a loss that stopped happening.
stale_row() {
    python3 - "$1" <<'PYEOF'
import sys

p = sys.argv[1] + "/tools/oracle-runner/src/trace.rs"
s = open(p, encoding="utf-8").read()
old = "        || id == cb::TAB_LIST\n"
assert s.count(old) == 1
open(p, "w", encoding="utf-8").write(s.replace(old, ""))
PYEOF
}

# A verdict outside the vocabulary.
bogus_verdict() {
    sed -i '0,/^verdict = "clock"$/s//verdict = "harmless"/' "$1/ci/checks/never-recorded.toml"
}

# The jar's answer and the ledger's disagree about who builds the packet. This
# is the `observed_by` discipline of the projection ledger, applied to a binary
# rather than to the check files.
sent_by_drift() {
    sed -i 's/net.minecraft.server.PlayerAdvancements/net.minecraft.server.PlayerAdvancementTracker/' \
        "$1/ci/checks/never-recorded.toml"
}

# `vacant` claimed for a packet the jar does build. The ledger's own half of
# the case above, kept because the two fail for different reasons and a reader
# should see both refusals.
vacant_but_sent() {
    python3 - "$1" <<'PYEOF'
import sys

p = sys.argv[1] + "/ci/checks/never-recorded.toml"
s = open(p, encoding="utf-8").read()
old = 'name = "KEEP_ALIVE"'
assert s.count(old) == 1
i = s.index(old)
j = s.index('verdict = "clock"', i)
open(p, "w", encoding="utf-8").write(s[:j] + 'verdict = "vacant"' + s[j + len('verdict = "clock"'):])
PYEOF
}

# The mirror: a packet ranked as sent that nothing builds. A row claiming a
# cost the jar does not support reads as coverage of a real loss and is not.
sent_but_vacant() {
    python3 - "$1" <<'PYEOF'
import sys

p = sys.argv[1] + "/ci/checks/never-recorded.toml"
s = open(p, encoding="utf-8").read()
i = s.index('name = "PING"')
j = s.index('verdict = "vacant"', i)
open(p, "w", encoding="utf-8").write(
    s[:j] + 'verdict = "recoverable"' + s[j + len('verdict = "vacant"'):]
)
PYEOF
}

# A class that moved under a version bump. Every verdict resting on it is
# stale, and the honest answer is a refusal rather than a scan that finds
# nothing and calls the packet vacant.
class_renamed() {
    sed -i 's/ClientboundUpdateAdvancementsPacket/ClientboundUpdateAdvancementPacket/' \
        "$1/ci/checks/never-recorded.toml"
}

# No jar. **The case this file exists to keep honest.** With the jar absent
# every scan returns the empty set, so every `vacant` row is satisfied and the
# check would print a green having read nothing — the exact defect that cost
# `ci/projection-drops.py::housekeeping_ids` its `housekeeping-kind` case.
absent_jar() {
    echo "/nonexistent/server-26.2.jar"
}

# A jar that is not the server jar. A truncated or wrong archive scans clean
# for the same reason an absent one does.
wrong_jar() {
    local jar
    jar=$(mktemp --suffix=.jar) || exit 2
    python3 - "$jar" <<'PYEOF'
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("a/b/C.class", b"\xca\xfe\xba\xbe")
PYEOF
    echo "$jar"
}

want 0 control                        control
want 1 vacant-gained-a-sender         vacant_gains_a_sender
want 1 unranked-housekeeping-kind     unranked_kind
want 1 stale-ledger-row               stale_row
want 1 verdict-outside-vocabulary     bogus_verdict
want 1 sent-by-drift                  sent_by_drift
want 1 vacant-but-sent                vacant_but_sent
want 1 sent-but-vacant                sent_but_vacant
want 2 class-renamed-in-jar           class_renamed
want 2 absent-jar                     absent_jar
want 2 wrong-jar                      wrong_jar

echo
echo "never-recorded-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
