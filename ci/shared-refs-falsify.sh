#!/usr/bin/env bash
# shared-refs-falsify.sh — nothing may delete a shared reference through a link.
#
#     ./ci/shared-refs-falsify.sh          # ~1 s, no server, no cargo, no lock
#
# # THE HAZARD
#
# 2.3 GB of vanilla references exist once, at `~/rustmc/oracle-work`, and every
# gate directory reaches them through SYMLINKS — that is what the setup ritual
# in `docs/AGENT-BRIEF.md` builds, and why a tree costs kilobytes instead of
# gigabytes. `rm -rf` follows a link's target rather than removing the link, so
# one script pointed at one of those links deletes the references for every
# agent on the box at once.
#
# `ci/t0-remote.sh`'s header is written around the version of this that HAS
# fired: an exclude pattern with a trailing slash replaced the remote's real
# `oracle-work` with a self-pointing symlink, and restoring it cost 200 MB over
# the wire and a re-extract of the structure templates.
#
# `ci/gen-vanilla-region.sh` had the un-fired version. It does `rm -rf "$DIR"
# "$OUT"` before harvesting, and `ci/t2-regiondiff.sh` passes it
# `oracle-work/t2-regiondiff/{a,b}` — which on this box is a symlink to the
# shared copy in **all 146 gate directories**, one harvest taken 2026-07-22 at
# 19:55 and byte-identical from every direction. It survived only because
# `ci/t2.sh` deliberately does not run that script. The first gate directory to
# run it would have deleted the harvest for the other 145.
#
#   1  a symlinked destination        -> REFUSED, exit 2, and the target survives
#   2  a real directory destination   -> not refused (the repair is not "never delete")
#   3  the refusal names the target    so the reader knows what was nearly lost
#
# Case 2 is the control. A guard that refuses everything would pass case 1 and
# make the script useless, and the two are indistinguishable from case 1 alone.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PASS=0; FAIL=0
ok() { printf '  ok    %-44s %s\n' "$1" "${2:-}"; PASS=$((PASS+1)); }
no() { printf '  FAIL  %-44s %s\n' "$1" "${2:-}"; FAIL=$((FAIL+1)); }

WORK="$(mktemp -d -t shared-refs-falsify.XXXXXX)" || exit 2
trap 'rm -rf "$WORK"' EXIT

# A stand-in for the shared reference tree, with something in it worth losing.
mkdir -p "$WORK/shared/precious"
echo "2.3 GB of vanilla, notionally" > "$WORK/shared/precious/region.mca"

# 0. WHAT `rm -rf` ACTUALLY DOES TO A SYMLINK, measured rather than assumed,
#    and BEFORE the guard is even looked for.
#
#    The intuitive answer is wrong and this case exists because an earlier
#    draft of the guard's comment gave the intuitive one. `rm -rf link` removes
#    the LINK; only a trailing slash reaches the target. So `ci/gen-vanilla-
#    region.sh` was never destroying the shared references, and a falsifier
#    that had asserted it was would have been green over a fiction.
#
#    The asymmetry is the finding: one character between "removes a link" and
#    "empties 2.3 GB that 146 trees share", in the same links and for the same
#    reason as the rsync exclude pattern in `ci/t0-remote.sh`'s header, which
#    HAS fired and cost 200 MB plus a re-extract.
mkdir -p "$WORK/haz/target" "$WORK/haz/target2"
echo "the shared reference" > "$WORK/haz/target/region.mca"
echo "the shared reference" > "$WORK/haz/target2/region.mca"
ln -sfn "$WORK/haz/target" "$WORK/haz/link"
rm -rf "$WORK/haz/link"
if [ -f "$WORK/haz/target/region.mca" ] && [ ! -e "$WORK/haz/link" ]; then
    ok "0a rm -rf <link> removes the link, not the target" "so this line was never the catastrophe"
else
    no "0a rm -rf <link> removes the link, not the target" "the platform disagrees; the guard's rationale must be re-derived"
fi
ln -sfn "$WORK/haz/target2" "$WORK/haz/link2"
rm -rf "$WORK/haz/link2/"
if [ ! -f "$WORK/haz/target2/region.mca" ]; then
    ok "0b rm -rf <link>/ empties the target" "one character; 146 trees share that target"
else
    no "0b rm -rf <link>/ empties the target" "the asymmetry the guard is a tripwire for does not exist here"
fi

# The guard, lifted from ci/gen-vanilla-region.sh so this file tests the code
# rather than a paraphrase of it. If the extraction finds nothing, that is a
# refusal: a falsifier silently testing its own copy of a guard is worse than
# no falsifier.
GUARD="$(sed -n '/^for _p in "\$DIR" "\$OUT"; do$/,/^done$/p' ci/gen-vanilla-region.sh)"
[ -n "$GUARD" ] || {
    echo "shared-refs-falsify REFUSED — no symlink guard found in ci/gen-vanilla-region.sh." >&2
    echo "    Either it was removed, or it was reworded and this extractor must follow it." >&2
    exit 2
}

run_guard() {  # $1 DIR, $2 OUT -> exit status of the guard
    ( DIR="$1"; OUT="$2"; eval "$GUARD" ) >"$WORK/out" 2>&1
}

# 1. The symlinked destination.
ln -sfn "$WORK/shared/precious" "$WORK/linked"
if run_guard "$WORK/realdir" "$WORK/linked"; then
    no "1 a symlinked destination -> REFUSED" "the guard allowed it"
else
    if [ -f "$WORK/shared/precious/region.mca" ]; then
        ok "1 a symlinked destination -> REFUSED" "and the shared target is untouched"
    else
        no "1 a symlinked destination -> REFUSED" "the target was deleted anyway"
    fi
fi

# 2. THE CONTROL. A guard that refuses everything is not a repair.
mkdir -p "$WORK/plain"
if run_guard "$WORK/otherplain" "$WORK/plain"; then
    ok "2 a real directory -> not refused" "regenerating a reference you own still works"
else
    no "2 a real directory -> not refused" "$(cat "$WORK/out")"
fi

# 3. The message has to name what was nearly lost, or the reader cannot tell a
#    link they meant to follow from one they did not know was there.
ln -sfn "$WORK/shared/precious" "$WORK/linked2"
run_guard "$WORK/realdir" "$WORK/linked2"
if grep -q "$WORK/shared/precious" "$WORK/out"; then
    ok "3 the refusal names the link's target"
else
    no "3 the refusal names the link's target" "$(cat "$WORK/out")"
fi

printf '\nshared-refs-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ] || exit 1
echo "SHARED-REFS-FALSIFY GREEN"
