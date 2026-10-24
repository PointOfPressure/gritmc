#!/usr/bin/env bash
# After the evidence recording: ship the final tip, re-run T0 remotely, push.
#
# `ci/run-pitch-evidence.sh` rsyncs once, at its own start, and everything it
# reports is about that snapshot. Four defects were found and fixed *after* it
# launched — `check-sound-pitch-table` renamed `.py` (T0 was red on the branch
# for it), `protocol_ids.json` added to the reference-cache key and to two
# fixtures, and `measured_reference_kinds` added to the report — so its T0
# stage is reporting a tree that no longer exists.
#
# This waits for that run to finish rather than racing it: rsyncing underneath
# a corpus run is the 2026-08-08 mtime hazard by another door, and the run is
# queued behind a merge gate that has had the box for twenty minutes.
#
# It waits by polling a log for a sentinel and exiting, which is the only
# sanctioned shape — no foreground `sleep` loop, and it matches the failure
# signatures as well as the success one, so a crash does not read as
# still-running.
set -u
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTMC_REMOTE_DIR=rustmc-pitch
R=~/$RUSTMC_REMOTE_DIR
EVIDENCE=~/grit-logs-pitch-evidence.log
LOG=~/grit-logs-projection-verify.log
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
HERE=$PWD

{
echo "=== $(date -Is) waiting for the evidence run to finish ==="
for _ in $(seq 1 360); do
    grep -qE 'DONE-PITCH-EVIDENCE|t2-gameplay exit [1-9]' "$EVIDENCE" 2>/dev/null && break
    sleep 30
done
grep -qE 'DONE-PITCH-EVIDENCE' "$EVIDENCE" 2>/dev/null \
    || echo "!!! evidence run did not reach DONE — verifying against it anyway, read $EVIDENCE"

echo "=== $(date -Is) stage 1: ship the final tip ==="
rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
    --exclude 'data/vanilla' "$HERE"/ threadripper:"$RUSTMC_REMOTE_DIR"/
# rsync -a preserves mtimes and cargo trusts them: a source older than the
# artefact built from it is silently reused, and the release binary then
# measures whatever tree built it while the log names this one.
ssh -n threadripper "cd $R && find crates tools ci -type f \
    \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \) -print0 | xargs -0 touch"

echo "=== $(date -Is) stage 2: T0 on the final tip ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && ./ci/t0.sh 2>&1 | tail -40"
echo "t0 exit $?"

echo "=== $(date -Is) stage 3: the projection identity this tip applies ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd $R && \
    cargo run -q -p oracle-runner --bin oracle-runner -- projection-id"
echo "floor declares: $(grep -m1 'projection = ' "$HERE"/ci/checks/gameplay-floor.toml)"

echo "=== $(date -Is) stage 4: what the recording says about the drops ==="
ssh -n threadripper "cd $R && python3 ci/observe-coverage.py oracle-work/gameplay 2>&1 | head -40"

echo "=== $(date -Is) stage 5: push ==="
cd "$HERE" || exit 2
git push --force-with-lease origin HEAD:corpus-projection-audit 2>&1 | tail -3
git push --force-with-lease origin HEAD:corpus-sound-pitch 2>&1 | tail -3
git ls-remote --heads origin corpus-projection-audit corpus-sound-pitch

echo "=== $(date -Is) DONE-PROJECTION-VERIFY ==="
} >"$LOG" 2>&1
