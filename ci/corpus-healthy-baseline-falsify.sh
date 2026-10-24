#!/usr/bin/env bash
# corpus-healthy-baseline-falsify.sh — the healthy-run control's own cases.
#
#     ./ci/corpus-healthy-baseline-falsify.sh
#
# `ci/corpus-healthy-baseline.sh` exists to say what a corpus run that is FINE
# looks like to the three instruments the 2026-08-07 overlap was diagnosed
# with. Its whole value is that the run it samples really is the one that owns
# the box. If it ever attaches to a queued run instead, it would report a
# process blocked on `flock` — no CPU, no log writes, two joins — and that
# reading would look exactly like the "stall" it exists to control against,
# while being a measurement of nothing.
#
# **CASE 3 IS THE ONE THAT MATTERS.** It is a status block with a holder and
# four waiters, and it requires the holder. A parser that matched " in <path>"
# without the trailing " since " passes every other case here and fails that
# one, which is precisely the bug worth having a falsifier for.
#
# Needs no lock, no box and no run: the status text is fed on stdin.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
B="$ROOT/ci/corpus-healthy-baseline.sh"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  ok    %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }
got() { "$B" --holder-from-status; }

echo "== corpus-healthy-baseline falsification"

# 1. A plain held status yields the holder's tree.
OUT="$(printf '%s\n' \
  'corpus lock: /home/sayer/.rustmc-corpus-lock' \
  '  state:   HELD' \
  '  record:  run pid 602906 (anchor 1904467) on sayer-desktop in /home/sayer/rustmc-corpusgap since 2026-08-18T09:52:30+01:00 (13m ago)' \
  | got)"
[ "$OUT" = /home/sayer/rustmc-corpusgap ] \
    && ok "1. a held lock yields the holder's tree" \
    || bad "1. expected /home/sayer/rustmc-corpusgap, got '$OUT'"

# 2. A free lock yields nothing, so the caller refuses rather than sampling
#    whatever process it can find.
OUT="$(printf '%s\n' 'corpus lock: /home/sayer/.rustmc-corpus-lock' '  state:   FREE' | got)"
[ -z "$OUT" ] && ok "2. a free lock yields no holder" || bad "2. a free lock yielded '$OUT'"

# 3. THE NEGATIVE CONTROL. Holder plus four waiters, each of which also names a
#    tree. Only the holder's line carries " since "; the waiters carry
#    ", waiting Nm". Picking a waiter here is the bug this file exists for.
OUT="$(printf '%s\n' \
  'corpus lock: /home/sayer/.rustmc-corpus-lock' \
  '  state:   HELD' \
  '  record:  run pid 602906 (anchor 1904467) on sayer-desktop in /home/sayer/rustmc-corpusgap since 2026-08-18T09:52:30+01:00 (13m ago)' \
  '  live fd holders (kernel truth, not the record):' \
  '    pid 1329417  bash ./ci/t2-gameplay.sh ' \
  '  gate intent: none' \
  '  waiting (service order — any gate first, then the oldest run):' \
  '    1. run pid 684068 in /home/sayer/rustmc-unported, waiting 108m' \
  '    2. run pid 1329417 in /home/sayer/rustmc-proto, waiting 54m' \
  '    3. run pid 1525319 in /home/sayer/rustmc-hold, waiting 34m' \
  '    4. run pid 1679534 in /home/sayer/rustmc-port, waiting 28m' \
  | got)"
if [ "$OUT" = /home/sayer/rustmc-corpusgap ]; then
    ok "3. NEGATIVE CONTROL — four waiters present, the holder is still chosen"
else
    bad "3. NEGATIVE CONTROL FAILED — picked '$OUT' instead of the holder"
fi

# 4. A waiting-only status — the lock was released between the two reads —
#    must yield nothing rather than the first waiter.
OUT="$(printf '%s\n' \
  '  state:   FREE' \
  '  waiting (service order):' \
  '    1. run pid 684068 in /home/sayer/rustmc-unported, waiting 108m' \
  | got)"
[ -z "$OUT" ] && ok "4. waiters without a holder yield nothing" || bad "4. yielded '$OUT'"

# 5. Empty input is not a holder.
OUT="$(printf '' | got)"
[ -z "$OUT" ] && ok "5. empty status yields nothing" || bad "5. yielded '$OUT'"

# 6. Two held records — impossible, but if the parser ever sees it, it must
#    take one deterministically rather than concatenating them into a path
#    that does not exist.
OUT="$(printf '%s\n' \
  '  record:  run pid 1 on h in /home/sayer/rustmc-one since 2026-08-18T09:52:30+01:00' \
  '  record:  run pid 2 on h in /home/sayer/rustmc-two since 2026-08-18T09:53:30+01:00' \
  | got)"
[ "$OUT" = /home/sayer/rustmc-one ] \
    && ok "6. two records yield the first, not a concatenation" \
    || bad "6. got '$OUT'"

# 7. The real lock, if one is held right now, must yield a directory that
#    exists. Skipped when the box is free, and a skip is printed rather than
#    silently counted as a pass.
REAL="$("$ROOT/ci/corpus-lock.sh" status 2>/dev/null | "$B" --holder-from-status)"
if [ -z "$REAL" ]; then
    echo "  skip  7. nobody holds the real lock right now — nothing to check against"
elif [ -d "$REAL" ]; then
    ok "7. the live lock's holder ($REAL) is a real directory"
else
    bad "7. the live lock yielded '$REAL', which is not a directory"
fi

echo
echo "corpus-healthy-baseline falsification: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
