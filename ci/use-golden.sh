#!/usr/bin/env bash
# use-golden.sh — extract `BlockState.useWithoutItem` per block state.
#
# `block_props.json` says `minecraft:lever` has a `powered` and so does
# `minecraft:stone_button`, and there is nothing in a table of *states* that
# says `LeverBlock` toggles it and holds, while `ButtonBlock` toggles it and
# then schedules a tick to toggle it back. This asks each state instead.
# See `tools/goldengen/vanilla/UseGolden.java` for what the probe is and what
# the world it clicks in looks like.
#
# One file, no shared pipeline: like `ci/placement-golden.sh` this needs no
# generated chunk, so compiling it beside `SurfaceChunkGolden` would buy
# nothing and cost the coupling.
#
# Same signature-stripping as `ci/placement-golden.sh`, for the same reason:
# the dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed jar.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# both streams through log4j, which re-emits each line with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so `grep -E "^SOMETHING"`
# matches nothing and exits 1 — indistinguishable from a clean run that found
# nothing. That prefix is also why this dumper writes its JSON to stdout and
# is captured with `>` rather than through a pipe.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/use_without_item.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

# The `instanceof ServerLevel` gates, derived from the bytecode and handed to
# the dumper. `instanceof` is answered by the JVM against the object's class, so
# `TableLevel` cannot observe that it failed one — the fact has to come from
# outside the run. `--emit` refuses if `ci/checks/server-level-gates.toml` does
# not describe what the jar actually gates, so a version bump stops here rather
# than writing a table with a stale gate column.
GATES="$ROOT/data/vanilla/server_level_gates.json"
mkdir -p "$(dirname "$GATES")"
"$ROOT/ci/server-level-gates.py" --emit >&2 || exit 1

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/UseGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    # stderr kept, not discarded. The dumper records a refusal when a block
    # reads something its level does not model — that is the design — but a
    # failure to *start* also lands here and the two look nothing alike.
    java -Drustmc.data="$VDIR/generated-data" -Drustmc.gates="$GATES" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.UseGolden > "$1" 2> "$1.log" || true
    # A `{` first byte, not merely a non-empty file: when the dumper throws
    # at top level, log4j has already claimed stdout and the "empty" file is
    # full of stack trace. `test -s` passes on it and the JSON parse fails
    # much later.
    if [ "$(head -c 1 "$1" 2>/dev/null)" != "{" ]; then
        echo "UseGolden did not produce JSON; the last of what it said:" >&2
        head -20 "$1" >&2
        tail -30 "$1.log" >&2
        return 1
    fi
}

# Twice, and they must agree — the same check `ci/placement-golden.sh` makes
# and for the same reason: `BuiltInRegistries.BLOCK` iterates in registration
# order and is stable, but a map iterated anywhere in the probe would not be,
# and that instability once churned the decorate vectors under a different
# dumper. Nothing here should depend on it; this is how that is checked
# rather than assumed.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of UseGolden disagree — something in it iterates a map" >&2
    # Say WHAT disagrees. A failure that reports only that two files differ
    # costs a full re-run with a hand-written diff to act on, and the answer
    # is usually one block: the dragon egg teleports itself to a random
    # position, and that is the shape of everything this check catches.
    echo "  sizes: $(stat -c%s "$WORK/out.json") and $(stat -c%s "$WORK/again.json")" >&2
    python3 - "$WORK/out.json" "$WORK/again.json" >&2 <<'PYDIFF'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
for k in a:
    if k != "table" and a[k] != b[k]:
        print(f"  summary field {k}: {a[k]!r} vs {b[k]!r}")
ra = {r["block"]: r for r in a["table"]}
rb = {r["block"]: r for r in b["table"]}
for k in sorted(set(ra) | set(rb)):
    if ra.get(k) != rb.get(k):
        print(f"  block {k} differs")
PYDIFF
    exit 1
}

# The regression oracle: `minecraft:lever` is the one block whose answer is
# already known end to end. It was ported by hand from `LeverBlock.pull`,
# and the corpus check `lever-flips-when-right-clicked` went `diverge` ->
# `match` against real vanilla on the strength of it. **If the extraction
# does not reproduce that, the extraction is wrong** — the hand port is the
# measured one. Checked here rather than left to a consumer, because a table
# that is wrong about the one block we can check is wrong about the fifty we
# cannot.
python3 - "$WORK/out.json" <<'PYEOF'
import json, sys
doc = json.load(open(sys.argv[1]))
rows = {r["block"]: r for r in doc["table"]}
lever = rows.get("minecraft:lever")
if lever is None:
    sys.exit("UseGolden recorded nothing for minecraft:lever, which is the one block we can check")
bad = []
for cell in lever["states"]:
    if "refused" in cell:
        bad.append(f"{cell['from']} refused: {cell['refused']}")
        continue
    powered = "powered=true" in cell["from"]
    writes = cell["writes"]
    if len(writes) != 1 or writes[0]["at"] != [0, 0, 0]:
        bad.append(f"{cell['from']} wrote {writes}, expected exactly one write at its own position")
        continue
    want = "powered=false" if powered else "powered=true"
    if want not in writes[0]["state"]:
        bad.append(f"{cell['from']} -> {writes[0]['state']}, expected {want}")
# **The dragon egg must come back random, every run.**
#
# It used to come back random *most* runs. Randomness was inferred by probing
# each state twice and asking whether the answers differed, and that is a coin
# flip when a rule has few outcomes: this harness's world is air everywhere,
# `DragonEggBlock.teleport` needs a target whose support is not air, and the
# only such square in the whole world is the one directly above the egg. So
# the search either finds it or exhausts its 1,000 attempts, and two probes
# agree whenever both succeed or both fail. Measured over six dumper runs on
# 2026-08-03: **twice out of six it reported "not random"**, which is a table
# that churns and a two-run `cmp` that fails with nothing changed.
#
# `TableLevel.getRandom` is instrumented now, so the answer is structural.
# This assertion is what stops it quietly going back to being a coin flip.
egg = rows.get("minecraft:dragon_egg")
if egg is None:
    bad.append("no row for minecraft:dragon_egg, which draws from the level RNG")
elif not egg["random"]:
    bad.append("dragon_egg is not flagged random — the RNG instrument is not firing, "
               "and this file will churn between runs")

if bad:
    print("UseGolden disagrees with the hand-ported, corpus-verified lever:", file=sys.stderr)
    for b in bad[:10]:
        print("  " + b, file=sys.stderr)
    sys.exit(1)
print(f"  lever oracle: {len(lever['states'])} states, all cycle `powered` in place")
print(f"  egg oracle:   dragon_egg flagged random by the RNG instrument")
PYEOF

# THE REFUSAL. A row whose answer is a reading of the probe's own player is
# this project's signature defect in a golden table: it cannot fail, because
# the only thing that could contradict it is a probe in another stance and until
# 2026-08-11 no such probe existed. `UseGolden.STANCES` sweeps eleven of them
# and sets `probeDependent`; this refuses to write the table when that column
# says something `ci/checks/use-probe-dependent.toml` does not already know.
#
# Deliberately BEFORE the `mv`, and deliberately not a warning. A warning on
# stdout is read by whoever is watching and by nobody afterwards, and the whole
# point is that the next block to read the player must not be able to enter the
# table while everyone is looking at something else.
"$ROOT/ci/check-use-probe.py" "$WORK/out.json" || exit 1

mv "$WORK/out.json" "$OUT"

# **Stamped, or a tree that followed the cold start fails T0.**
# `ci/check-use-probe-falsify.sh`'s `real-table` arm refuses a table with no
# provenance stamp and points at `ci/gen-gameplay-tables.sh` to repair it.
# `ci/use-item-on-golden.sh` grew this on 2026-08-18 for exactly that reason
# and the other two generators did not, so any tree that ran *this* script
# directly — which is what every cold start in `docs/cold-start/` tells you to
# do — had an unstamped table and a red gate whose advice was to run a
# different script. Found 2026-08-19 by following `server-level-gate.md`'s own
# command list from a fresh worktree.
if [ -f "$ROOT/ci/provenance.sh" ]; then
    . "$ROOT/ci/provenance.sh"
    prov_stamp "$ROOT" "$OUT" "ci/use-golden.sh"
fi
python3 - "$OUT" <<'PYEOF'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"use table -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['blocksWithBehaviour']} blocks with behaviour, "
      f"{d['statesWriting']} of {d['statesProbed']} states write something, "
      f"{d['refusals']} refusals")
print(f"  {d['serverLevelGatedCells']} cell(s) over {d['serverLevelGatedBlocks']} "
      f"block(s) stamped `gated` — the row says which column the "
      f"`instanceof ServerLevel` branch decided")
for k, v in list(d["refusedBy"].items())[:8]:
    print(f"    refused {v:5}  {k}")
PYEOF
