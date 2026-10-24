#!/usr/bin/env bash
# menu-probe.sh — which of vanilla's 25 `minecraft:menu` types can this
# substrate actually open, and does each one record the same thing twice?
#
#     ./ci/menu-probe.sh              # two cold vanilla boots
#     MENU_PROBE_RUNS=3 ./ci/menu-probe.sh
#
# `docs/PARITY-ROADMAP.md` §21 measures the gameplay corpus against vanilla's
# own registries and reports `menu 3 / 25`. The three are the chest, the
# furnace and the crafting table. Every other menu is opened by a block this
# corpus has never placed or by an entity it cannot spawn, so the question
# before writing seventeen checks is not "does our server do it" but **"does
# vanilla answer at all, and does it answer the same way twice"**.
#
# This is the cheap half of that. It boots vanilla on its own port, records
# only the menu checks, and boots again from scratch for the second recording
# so the two are as independent as two `ci/t2-gameplay.sh` runs are — a second
# `record` against a server that is already up would share its world, its
# block entities and its container counter, and a scenario that flapped on a
# cold boot would look stable.
#
# It writes nothing into the corpus and moves no baseline. **It does take the
# box-wide corpus lock, as role `run`**, and that is a deliberate change from
# `ci/finish-dig-probe.sh`, which takes none: this boots a vanilla server and
# records eighteen scenarios through `settle_ms` windows, which is exactly the
# shape of work `ci/corpus-lock.sh` exists to serialise, and a recording taken
# beside another corpus run is a recording whose timing is another run's. The
# floor this feeds would then be measuring contention. **The window is one
# vanilla boot plus about 18 x 13 s per run — under five minutes at
# `MENU_PROBE_RUNS=2`** — and role `run` means a real merge gate still jumps
# ahead. The floor it feeds is
#
#     ./ci/corpus-floor.sh --checks <the subset> --no-adjudicate r1.json r2.json
#
# which is the instrument that decides whether a new check is written or is
# reference-unstable. Read `ci/corpus-floor.py`'s header for what that says.
#
# # What to read in the output
#
# Per scenario: whether an `open_screen` arrived, the `minecraft:menu`
# protocol id it named, and the digest of the rest of the body — which is the
# menu's **title**. A menu with no `open_screen` did not open, and the block
# is then a substrate finding rather than a check. Two scenarios sharing an
# `open_screen` digest are two checks that cannot tell each other apart, which
# is the defect these checks exist to avoid: the blast furnace and the smoker
# have the furnace's slot layout exactly and differ from it only in the
# registry id and the title.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${MENU_PROBE_PORT:-25898}"
RUNS="${MENU_PROBE_RUNS:-2}"
WORK="$ROOT/oracle-work/gameplay/menu-probe"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

# Sourced, not run: `corpus_lock_acquire` has to hold the descriptor in *this*
# shell or the kernel drops it when the subshell exits.
. "$ROOT/ci/corpus-lock.sh"

VAN_DIR=""
cleanup() {
    [ -n "$VAN_DIR" ] && "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
}
trap cleanup EXIT

corpus_lock_acquire

rm -rf "$WORK"
mkdir -p "$SCEN"

# The subset is the corpus's own files, copied rather than rewritten: a probe
# that measured a private copy of a check would be measuring the copy. `470`
# rides along as the control the blast furnace and the smoker are compared
# against — without it, "these three differ" is a claim about three unread
# digests.
cp "$ROOT/ci/checks/gameplay/470-a-furnace-opens-a-screen-a-chest-does-not.toml" "$SCEN/"
cp "$ROOT"/ci/checks/gameplay/7[45][0-9]-*.toml "$SCEN/"
ls "$SCEN" | sed 's/^/  subset: /'

cargo build -q -p oracle-runner --release

for run in $(seq 1 "$RUNS"); do
    VAN_DIR="$WORK/vanilla-$run"
    echo "== run $run/$RUNS: cold vanilla on :$PORT"
    VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$PORT" "minecraft:flat" "$SEED"
    for r in advance_time spawn_mobs random_tick_speed advance_weather; do
        case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
        "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
    done
    sleep 2
    "$ROOT/target/release/oracle-runner" record "127.0.0.1:$PORT" "$SCEN" \
        --out "$WORK/r$run.json"
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
    VAN_DIR=""
done

python3 - "$ROOT/oracle-work/vanilla-26.2/generated/reports/registries.json" \
         "$WORK"/r*.json <<'PY'
import json, sys, collections

regs = json.load(open(sys.argv[1]))["minecraft:menu"]["entries"]
by_id = {v["protocol_id"]: k for k, v in regs.items()}

runs = [json.load(open(p)) for p in sys.argv[2:]]
names = [s["name"] for s in runs[0]["scenarios"]]

print(f"\n== {len(runs)} recordings, {len(names)} scenarios, "
      f"denominator {len(by_id)} menu types ==\n")

seen = {}
for name in names:
    cells = []
    err = None
    for r in runs:
        s = next(x for x in r["scenarios"] if x["name"] == name)
        err = err or s.get("error")
        obs = [o for o in s["obs"] if o["who"] == "actor"]
        op = [o for o in obs if o["kind"] == "open_screen"]
        co = [o for o in obs if o["kind"] == "container_set_content"]
        # **Only the two kinds these checks declare.** Comparing every kind
        # the actor heard makes this column read `move_entity_heartbeat`,
        # which is a server timer and not a menu: the first version of this
        # script did that and called thirteen scenarios unstable, including
        # `470`, which the corpus has held at `match` since 2026-08-04. The
        # instrument that adjudicates stability is `ci/corpus-floor.sh` and
        # it projects through each check's own `observe` list; this column is
        # a cheap agreement with it, so it has to project the same way.
        cells.append((
            tuple((o["fields"].get("menu"), o["fields"].get("rest")) for o in op),
            tuple(o["fields"].get("rest") for o in co),
        ))
    stable = all(c == cells[0] for c in cells)
    op, co = cells[0]
    # A scenario the harness refused is not a scenario that opened no menu,
    # and printing them the same way cost a turn: `minecraft:smithing` is not
    # an item, the recorder said so in the trace, and the first run of this
    # script reported it as a smithing table that declines to open.
    if err:
        print(f"  {'ERROR':10s} {name}: {err}")
        continue
    if not op:
        print(f"  {'NO MENU':10s} {name}")
        continue
    for mid, rest in op:
        seen.setdefault(by_id.get(mid, f"id {mid}"), name)
        print(f"  {'stable' if stable else 'FLAPS':10s} {name}")
        print(f"             menu={mid} {by_id.get(mid, '??')}  "
              f"title-digest={rest}  content-digest={co}")

print(f"\n-- menu types this subset reached: {len(seen)} of {len(by_id)}")
for k, v in sorted(seen.items()):
    print(f"     {k:34s} {v}")

# Two checks that project the same bytes are one check twice. The furnace
# control is in the subset for exactly this comparison.
dupes = collections.defaultdict(list)
for name in names:
    s = next(x for x in runs[0]["scenarios"] if x["name"] == name)
    for o in s["obs"]:
        if o["who"] == "actor" and o["kind"] == "open_screen":
            dupes[(o["fields"].get("menu"), o["fields"].get("rest"))].append(name)
clash = {k: v for k, v in dupes.items() if len(v) > 1}
print(f"\n-- open_screen projections shared by more than one scenario: {len(clash)}")
for k, v in clash.items():
    print(f"     {k} {v}")

# Refuse rather than print. A recorder error is a harness fault, and a probe
# that reports one as a substrate finding is the instrument this project has
# been caught by twenty-one times.
bad = [s["name"] for r in runs for s in r["scenarios"] if s.get("error")]
if bad:
    print(f"\nMENU PROBE REFUSED — the recorder errored on {sorted(set(bad))}")
    sys.exit(2)
PY
