#!/usr/bin/env bash
# The one recording session this branch takes, and everything that depends on it.
#
# Queued ONCE, at the end, on the argument `ci/run-pitch-evidence.sh` makes:
# the corpus lock is the scarcest resource on this box and two agents starved
# on it for 184 and 171 minutes on 2026-08-17. Everything answerable from the
# tree was answered before this ran — the ledger's four ranks, the `stage`
# reclassification, the never-recorded verdicts, all of `ci/never-recorded.py`.
#
# # THE ORDER IS DELIBERATE: THE LOCK-FREE MEASUREMENT GOES FIRST
#
# Stage 5 is the deliverable — `minecraft:player`'s `add_entity.movement`, the
# half of that drop `f91984bb` named and could not measure — and
# `ci/entity-spawn-probe.sh` **does not take the box-wide corpus lock**. Stage 6
# does, and may queue behind production merges for hours. Running them the other
# way round would make the answerable question wait on the contended one, which
# is how a lane ends a session with a starved lock and nothing measured.
#
# Chained end to end so the agent does not stop between stages.
set -u
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTMC_REMOTE_DIR=rustmc-proj2
R=~/$RUSTMC_REMOTE_DIR
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
HERE=$PWD
OUT=~/grit/logs/proj2-evidence-$(date +%Y%m%d-%H%M%S)
mkdir -p "$OUT"

echo "=== $(date -Is) stage 1: ship the branch tip to $R ==="
rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
    --exclude 'data/vanilla' "$HERE"/ threadripper:"$RUSTMC_REMOTE_DIR"/ || exit 2
# `rsync -a` preserves mtimes and cargo trusts mtimes: a source older than the
# artefact built from it is silently reused. AGENT-BRIEF, 2026-08-08.
ssh -n threadripper "cd $R && find crates tools ci -type f \
    \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \) -print0 | xargs -0 touch"
# oracle-work is excluded from the rsync and every stage below reads the jar.
ssh -n threadripper "cd $R && [ -e oracle-work ] || ln -s ~/rustmc/oracle-work oracle-work; ls -d oracle-work/vanilla-26.2 >/dev/null" \
    || { echo "no oracle-work reachable from $R"; exit 2; }

echo "=== $(date -Is) stage 2: data/vanilla, retried rather than diagnosed ==="
# `ci/gen-gameplay-tables.sh` boots a real vanilla server and loses to whatever
# else on this box holds its port. `ci/entity-movement-report.py` needs
# `data/vanilla` to turn a `kind_id` into a name, and crashed on 2026-08-18
# after twenty minutes of recording because it was absent.
for attempt in 1 2 3 4 5 6; do
    if ssh -n threadripper "test -s $R/data/vanilla/recipe_unlocks.json"; then
        echo "data/vanilla present (attempt $attempt)"; break
    fi
    echo "--- attempt $attempt $(date -Is)"
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
        ./ci/gen-gameplay-tables.sh 2>&1 | tail -12"
    sleep 90
done

echo "=== $(date -Is) stage 3: release build ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && cargo build --release 2>&1 | tail -20"
echo "build exit $?"

echo "=== $(date -Is) stage 4: T0 on the committed tree ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && ./ci/t0.sh 2>&1 | tail -30"
echo "t0 exit $?"

echo "=== $(date -Is) stage 5: THE MEASUREMENT — minecraft:player's movement, N=6, lock-free ==="
# The probe's own corpus plus the two checks that put a PLAYER in a measured
# window. `530` is the one that matters: its actor walks out of tracking range
# and back, so the standing player is re-announced with an `add_entity` and the
# movement field is the one `getLastSentMovement()` filled. `080` is carried as
# a second route to the same type through `PlayerList.placeNewPlayer`.
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
    ENTITY_RUNS=6 ENTITY_PROBE_PORT=25917 \
    ENTITY_PROBE_EXTRA='ci/checks/gameplay/530-walking-away-drops-a-standing-player-from-view.toml ci/checks/gameplay/080-other-player-is-an-entity.toml' \
    ./ci/entity-spawn-probe.sh 2>&1 | tail -160"
echo "entity-spawn-probe exit $?"

echo "=== $(date -Is) stage 5b: the movement report on its own, kept ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
    ./ci/entity-movement-report.py oracle-work/gameplay/entity-probe/r*.json 2>&1" \
    | tee "$OUT/entity-movement-player.log"

echo "=== $(date -Is) stage 5c: every add_entity carrying a player, raw ==="
ssh -n threadripper "cd $R && python3 - oracle-work/gameplay/entity-probe/r*.json <<'PY'
import json, sys, collections
# The player type id, read from the generated registry rather than written
# down: a literal here would be a fourth place the number lives.
try:
    reg = json.load(open('data/vanilla/entity_types.json'))['entity_types']
    pid = reg['minecraft:player']
except Exception as e:
    pid = f'<unresolved: {e}>'
seen = collections.defaultdict(list)
for p in sys.argv[1:]:
    d = json.load(open(p))
    for sc in d.get('scenarios', []):
        for o in sc.get('obs', []):
            if o.get('kind') != 'add_entity':
                continue
            f = o['fields']
            seen[(sc['name'], f.get('kind_id'))].append(f.get('movement', '<ABSENT>'))
print('player type id from registry:', pid)
for (name, kid), vals in sorted(seen.items(), key=lambda x: str(x[0])):
    distinct = {json.dumps(v) for v in vals}
    print(f'{name[:46]:<48} kind_id={kid!s:<5} n={len(vals):<3} distinct={len(distinct)}')
    for v in sorted(distinct)[:6]:
        print('        ', v[:110])
PY" | tee "$OUT/player-add-entity-raw.log"

echo "=== $(date -Is) stage 6: the corpus recording (TAKES THE LOCK, waits its turn) ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && ./ci/t2-gameplay.sh 2>&1 | tail -140"
echo "t2-gameplay exit $?"

echo "=== $(date -Is) stage 7: re-sweep the falsifiability ledger, 115 scenarios ==="
# The ledger on this branch is `entity-parity`'s sweep of 115 scenarios, taken
# before `sound.pitch` and `add_entity.movement` were recorded fields. Both are
# perturbable now, so the mutation set is wider than the one that produced it.
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
    ./ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner \
    oracle-work/gameplay/vanilla.json oracle-work/gameplay/rustmc.json 2>&1 | tail -50"
echo "sweep exit $?"

echo "=== $(date -Is) stage 8: bring the artefacts home, commit nothing ==="
# Copied, not committed. `ci/run-pitch-evidence.sh` committed its ledger from
# inside the script and the commit message it carried was written before the
# measurement existed. A message that predates its own evidence is the thing
# this repository keeps refusing.
scp threadripper:"$R"/ci/checks/gameplay-falsifiable.toml "$OUT"/gameplay-falsifiable.toml \
    && echo "ledger fetched to $OUT/gameplay-falsifiable.toml"
ssh -n threadripper "cd $R && ./target/release/oracle-runner projection-id" \
    | tee "$OUT/projection-id.txt"
ssh -n threadripper "cd $R && ./target/release/oracle-runner recorder-id" \
    | tee "$OUT/recorder-id.txt"

echo "=== $(date -Is) DONE — artefacts in $OUT ==="
ls -la "$OUT"
