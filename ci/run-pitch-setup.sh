#!/usr/bin/env bash
# One-shot: set this branch's remote dir up, build, and run T0.
#
# Chained rather than stepped so the agent does not stop between stages
# (BRIEF-ADDENDUM: each resume re-reads the whole transcript). Every stage
# guards, logs and continues so one failure does not strand the rest.
#
# Takes NO corpus lock. Nothing here records.
set -u
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTMC_REMOTE_DIR=rustmc-pitch
LOG=~/grit-logs-pitch-setup.log
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2

{
echo "=== $(date -Is) stage 1: t0-remote ships the tree and the sibling plugins ==="
./ci/t0-remote.sh; T0=$?
echo "t0-remote exit $T0"

echo "=== $(date -Is) stage 2: own oracle-work, own data/vanilla ==="
ssh threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; \
   mkdir -p ~/$RUSTMC_REMOTE_DIR/oracle-work/gameplay \
       ~/$RUSTMC_REMOTE_DIR/oracle-work/gameplay-selfcheck ~/$RUSTMC_REMOTE_DIR/data/vanilla; \
   for e in ~/rustmc/oracle-work/*; do case \$(basename \$e) in gameplay*) continue;; esac; \
       ln -sfn \$e ~/$RUSTMC_REMOTE_DIR/oracle-work/; done; \
   cp -rn ~/rustmc/data/vanilla/. ~/$RUSTMC_REMOTE_DIR/data/vanilla/; \
   cd ~/$RUSTMC_REMOTE_DIR && ./ci/gen-gameplay-tables.sh"
echo "gen-gameplay-tables exit $?"

echo "=== $(date -Is) stage 3: mtimes forward, then build ==="
ssh threadripper "cd ~/$RUSTMC_REMOTE_DIR && find crates tools ci -type f \
    \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \) -print0 | xargs -0 touch"
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd ~/$RUSTMC_REMOTE_DIR && \
    cargo build --release -p oracle-runner"
echo "build exit $?"

echo "=== $(date -Is) stage 4: oracle-runner unit tests ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd ~/$RUSTMC_REMOTE_DIR && \
    cargo test --release -p oracle-runner --bins -- --nocapture pitch 2>&1 | tail -40"
echo "pitch tests exit $?"

echo "=== $(date -Is) stage 5: full T0 on the committed tree ==="
ssh -n threadripper "export PATH=\$HOME/.cargo/bin:\$PATH; cd ~/$RUSTMC_REMOTE_DIR && ./ci/t0.sh 2>&1 | tail -60"
echo "t0 exit $?"

echo "=== $(date -Is) DONE-PITCH-SETUP ==="
} >"$LOG" 2>&1
