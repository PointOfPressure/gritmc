#!/usr/bin/env bash
# The one recording this branch takes, and everything that depends on it.
#
# Queued ONCE, at the end, because the corpus lock is the scarcest resource on
# this box — two agents starved on it for 184 and 171 minutes on 2026-08-17.
# Everything that could be done without a recording was done before this ran.
# `ci/handoff.sh` is deliberately NOT used: it declares into the lock as
# role=gate and would compete with production merges as an equal.
#
# Chained end to end so the agent does not stop between stages.
set -u
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTMC_REMOTE_DIR=rustmc-pitch
R=~/$RUSTMC_REMOTE_DIR
LOG=~/grit-logs-pitch-evidence.log
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
HERE=$PWD

{
echo "=== $(date -Is) stage 1: ship the branch tip ==="
rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
    --exclude 'data/vanilla' "$HERE"/ threadripper:"$RUSTMC_REMOTE_DIR"/
# rsync -a preserves mtimes and cargo trusts mtimes: a source older than the
# artefact built from it is silently reused. AGENT-BRIEF, 2026-08-08.
ssh -n threadripper "cd $R && find crates tools ci -type f \
    \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \) -print0 | xargs -0 touch"

echo "=== $(date -Is) stage 1b: finish data/vanilla ==="
# `ci/dump-recipe-unlocks.sh` boots a real vanilla server, so it loses to
# whatever else on this box has :25750 — which is what happened at 20:46 on
# 2026-08-17 and left the tree half-generated. T0 then REFUSES rather than
# measuring, correctly, and the refusal reads like a branch defect in a branch
# that touched none of it. Retried rather than diagnosed again.
for attempt in 1 2 3 4 5 6; do
    if ssh -n threadripper "test -s $R/data/vanilla/recipe_unlocks.json"; then
        echo "recipe_unlocks.json present (attempt $attempt)"; break
    fi
    echo "--- attempt $attempt $(date -Is)"
    ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
        ./ci/gen-gameplay-tables.sh 2>&1 | tail -12"
    sleep 90
done

echo "=== $(date -Is) stage 2: release build ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && cargo build --release"
echo "build exit $?"

echo "=== $(date -Is) stage 2b: T0 on the committed tree ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && ./ci/t0.sh 2>&1 | tail -25"
echo "t0 exit $?"

echo "=== $(date -Is) stage 3: the recording (takes the corpus lock, waits its turn) ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && ./ci/t2-gameplay.sh 2>&1 | tail -120"
echo "t2-gameplay exit $?"

echo "=== $(date -Is) stage 4: did the pitch actually reach the recording ==="
ssh -n threadripper "cd $R && python3 - <<'PY'
import json, collections
for side in ('vanilla', 'rustmc'):
    try:
        d = json.load(open(f'oracle-work/gameplay/{side}.json'))
    except Exception as e:
        print(side, 'MISSING', e); continue
    seen, withp = collections.Counter(), collections.Counter()
    for sc in d.get('scenarios', []):
        for o in sc.get('obs', []):
            if o.get('kind') == 'sound':
                sid = o['fields'].get('sound'); seen[sid] += 1
                if 'pitch' in o['fields']: withp[sid] += 1
    print(f'{side}: {sum(seen.values())} sound observations, '
          f'{sum(withp.values())} carry a pitch, '
          f'{len(withp)}/{len(seen)} distinct events classified')
PY"

echo "=== $(date -Is) stage 5: the lever and the comparator, side by side ==="
ssh -n threadripper "cd $R && python3 - <<'PY'
import json
want = {935: 'block.lever.click', 383: 'block.comparator.click'}
for side in ('vanilla', 'rustmc'):
    try:
        d = json.load(open(f'oracle-work/gameplay/{side}.json'))
    except Exception:
        continue
    for sc in d.get('scenarios', []):
        for o in sc.get('obs', []):
            if o.get('kind') == 'sound' and o['fields'].get('sound') in want:
                print(side, sc['name'][:44], want[o['fields']['sound']],
                      'pitch=', o['fields'].get('pitch', '<ABSENT>'))
PY"

echo "=== $(date -Is) stage 6: re-sweep — perturb:pitch is the red-when-broken evidence ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
    ./ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner \
    oracle-work/gameplay/vanilla.json oracle-work/gameplay/rustmc.json 2>&1 | tail -40"
echo "sweep exit $?"

echo "=== $(date -Is) stage 7: bring the ledger home ==="
scp threadripper:"$R"/ci/checks/gameplay-falsifiable.toml "$HERE"/ci/checks/gameplay-falsifiable.toml \
    && echo "ledger copied"
cd "$HERE" && git add ci/checks/gameplay-falsifiable.toml && \
    git commit -q -m 'ci: re-sweep the falsifiability ledger with sound.pitch recorded

The sweep plans one perturb mutation per field present in the subject at the
measured step, so `pitch` becoming a recorded field makes `perturb:pitch` a
live mutation for every scenario whose measured step emits a sound. A kill by
it is the red-when-broken evidence for this branch: the subject emits a
different pitch and the scenario outcome moves, which before today it could
not, because the field was not in the trace to perturb.

Recorded on threadripper in ~/rustmc-pitch, one corpus run.' && echo "ledger committed"

echo "=== $(date -Is) stage 6b: the floor, re-measured under the new projection ==="
# `pitch.rs` is a projection source, so `proj:` moved and
# `ci/checks/gameplay-floor.toml` reads STALE PROJECTION. The floor may only be
# re-taken over recordings THIS recorder made: every `vanilla.json` older than
# this branch carries no `pitch` field, the new comparator is inert over it,
# and a floor stamped that way certifies an instrument it never exercised —
# `ci/trace_vocabulary.py` §15.4. So exactly one recording is admitted here,
# and the resolving power is printed beside the floor so an N of 1 cannot read
# as a clean measurement.
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
    ./ci/corpus-floor.sh oracle-work/gameplay/vanilla.json 2>&1 | tail -30"
echo "corpus-floor exit $?"

echo "=== $(date -Is) stage 6c: what the discarded client says, on a fresh recording ==="
ssh -n threadripper "cd $R && python3 ci/hidden-stream-audit.py oracle-work/gameplay 2>&1 | head -60"

echo "=== $(date -Is) stage 6d: the verdicts this branch can move ==="
ssh -n threadripper "cd $R && python3 - <<'PY'
import json
want = {'a-comparator-switches-to-subtract-mode', 'a-note-block-retunes-when-right-clicked',
        'attacking-another-player', 'attacking-from-out-of-range',
        'a-fence-gate-opens-when-right-clicked',
        'a-fence-gate-opened-from-behind-turns-to-face-you'}
try:
    r = json.load(open('oracle-work/gameplay/report.json'))
except Exception as e:
    print('no report:', e); raise SystemExit
for o in r['outcomes']:
    if o['name'] in want:
        print(f\"{o['name'][:48]:<50} declared={o['verdict']:<9} strict={o.get('strict')} \"
              f\"ref_obs={o.get('measured_reference_obs')}\")
PY"

echo "=== $(date -Is) stage 8: push ==="
git push --force-with-lease origin HEAD:corpus-sound-pitch 2>&1 | tail -5
git ls-remote --heads origin corpus-sound-pitch

echo "=== $(date -Is) DONE-PITCH-EVIDENCE ==="
} >"$LOG" 2>&1
