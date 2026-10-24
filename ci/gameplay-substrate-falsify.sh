#!/usr/bin/env bash
# gameplay-substrate-falsify.sh — the gate on `ci/gameplay-substrate.sh`.
#
#     ./ci/gameplay-substrate-falsify.sh
#
# No box, no corpus lock, no server, no cargo. Under a second.
#
# # WHAT IT HAS TO PROVE
#
# The shipped defect was **one-sided quieting**: `ci/t2-gameplay.sh` sent four
# game rules to vanilla AND to this server, and `ci/t2-gameplay-subset.sh` sent
# them to vanilla only — it passed no `--console` to `rustmc-server`, so it
# could not have sent them at all. The subject random-ticked, advanced its
# clock and cycled its weather through every subset run this project has taken,
# while the reference did none of those things.
#
# The defect had a second half that made it durable: **the list was written out
# twice**, so the two scripts could drift and the reference cache's `harness`
# component, which hashes `ci/t2-gameplay.sh`, would not notice.
#
# So three things are asserted, and each has an injection that breaks it:
#
#   1. neither script carries a literal rule list — both read the array;
#   2. each script delivers the array to BOTH servers;
#   3. the reference cache's key covers the file the array lives in, because
#      "change a rule, change the substrate, miss the cache" was written beside
#      the array and moving the array without moving the key would have made
#      that sentence false.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  ok    %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$1"; }

code() { grep -vE '^[[:space:]]*#' "$1"; }

# --- 1. one list, in one file ----------------------------------------------

n="$(code ci/gameplay-substrate.sh | grep -cE '^SUBSTRATE_GAMERULES=\(')"
[ "$n" = 1 ] && ok "ci/gameplay-substrate.sh declares the array exactly once" \
             || bad "ci/gameplay-substrate.sh declares the array $n times"

# The literal rule names must not appear as a list in either harness script.
# One name in prose is fine; two or more on one non-comment line is a second
# copy of the list.
literal() {
    code "$1" | grep -cE 'advance_time.*spawn_mobs|spawn_mobs.*random_tick_speed|random_tick_speed.*advance_weather' || true
}
for f in ci/t2-gameplay.sh ci/t2-gameplay-subset.sh; do
    c="$(literal "$f")"
    [ "$c" = 0 ] && ok "$f carries no literal rule list" \
                 || bad "$f carries $c literal rule list(s) — a second copy that can drift"
    if grep -q '\. "\$ROOT/ci/gameplay-substrate.sh"' "$f"; then
        ok "$f sources ci/gameplay-substrate.sh"
    else
        bad "$f does not source ci/gameplay-substrate.sh"
    fi
done

# The injection: put the list back inline and the check must catch it.
cp ci/t2-gameplay-subset.sh "$T/inj-list.sh"
printf 'for r in advance_time spawn_mobs random_tick_speed advance_weather; do :; done\n' \
    >> "$T/inj-list.sh"
[ "$(literal "$T/inj-list.sh")" != 0 ] \
    && ok "injection: an inlined rule list is caught" \
    || bad "injection: an inlined rule list is NOT caught"

# --- 2. both servers, in both scripts --------------------------------------
#
# The reference is quieted through `$VAN_DIR` and the subject through
# `$OUR_DIR`; both go via `ci/vanilla-server.sh console`, which is why one
# grep can ask the question of both.
for f in ci/t2-gameplay.sh ci/t2-gameplay-subset.sh; do
    v="$(code "$f" | grep -cE 'gamerule.*\$\{gr%%=\*\}|gamerule \$rule' || true)"
    # Two shapes, because the gate wraps its reference delivery in
    # `gamerule_or_die "$VAN_DIR" …` while the subset calls
    # `ci/vanilla-server.sh console "$VAN_DIR"` directly. Both are a console
    # command to the reference; asking only for the second one reported the
    # gate as never quieting vanilla, which this suite did on its first run.
    van="$(code "$f" | grep -cE 'console "\$VAN_DIR"|gamerule_or_die "\$VAN_DIR"' || true)"
    our="$(code "$f" | grep -c 'console "\$OUR_DIR"' || true)"
    [ "$van" -ge 1 ] && ok "$f delivers game rules to vanilla" \
                     || bad "$f never sends a console command to \$VAN_DIR"
    [ "$our" -ge 1 ] && ok "$f delivers game rules to the subject" \
                     || bad "$f never sends a console command to \$OUR_DIR — THE SHIPPED DEFECT"
    [ "$v" -ge 1 ]   && ok "$f's delivery reads the array, not a literal" \
                     || bad "$f's delivery does not expand SUBSTRATE_GAMERULES"
    # A subject that is never given a console cannot be quieted whatever the
    # loop says, so the flag is part of the assertion rather than beside it.
    # `grep -c`, never `grep -q`, after a pipe under `pipefail`: `-q` exits
    # at the first match, `code`'s grep takes SIGPIPE writing the rest, and
    # the pipeline reports 141. Under load that read as "no --console" in
    # 10,218 of 36,000 trials (24 niced loops, 2026-09-24) and redded
    # gate-instruments' gate at 6cd236f54 on an unchanged ci/t2-gameplay.sh.
    if [ "$(code "$f" | grep -c -- '--console "\$OUR_DIR/console.fifo"' || true)" -gt 0 ]; then
        ok "$f gives the subject a console"
    else
        bad "$f does not pass --console to rustmc-server, so it cannot quiet it"
    fi
done

cp ci/t2-gameplay-subset.sh "$T/inj-onesided.sh"
sed -i 's|console "\$OUR_DIR"|console "$VAN_DIR"|g' "$T/inj-onesided.sh"
[ "$(code "$T/inj-onesided.sh" | grep -c 'console "\$OUR_DIR"' || true)" = 0 ] \
    && ok "injection: quieting only vanilla is caught" \
    || bad "injection: quieting only vanilla is NOT caught"

# --- 3. the cache key covers the substrate ---------------------------------
#
# `ci/t2-gameplay.sh` said, beside the array, "change a rule, change the
# substrate, miss the cache, boot vanilla". That was true while the array lived
# in a file the key hashes. Moving it out without moving the key would have
# left a rule edit invisible to the cache, which serves a reference recorded
# under the OLD substrate — a cached reference of a different world.
for f in ci/gameplay-substrate.sh ci/gameplay-difficulty.sh; do
    if grep -q "$f" ci/reference-cache.sh; then
        ok "ci/reference-cache.sh's key covers $f"
    else
        bad "ci/reference-cache.sh's key does not cover $f — a rule or difficulty edit would not miss the cache"
    fi
done

# And the VALUE, not only the file: `GAMEPLAY_DIFFICULTY` is an environment
# variable, so hashing its file covers a change to the default and nothing
# else.
. "$ROOT/ci/reference-cache.sh"
A="$(GAMEPLAY_DIFFICULTY=peaceful refcache_components "$ROOT" /nonexistent 20260803 2>/dev/null | grep '^difficulty=' || true)"
B="$(GAMEPLAY_DIFFICULTY=easy     refcache_components "$ROOT" /nonexistent 20260803 2>/dev/null | grep '^difficulty=' || true)"
[ -n "$A" ] && [ "$A" != "$B" ] \
    && ok "the cache key moves with GAMEPLAY_DIFFICULTY ($A vs $B)" \
    || bad "the cache key does not move with GAMEPLAY_DIFFICULTY — a peaceful reference would be served to an easy run"

echo
echo "gameplay-substrate-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
