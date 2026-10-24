#!/usr/bin/env bash
# bench-stamp-sha.sh — stamp a built bench directory with the source sha it
# was actually built from, verified by diffing its tree against `git archive
# <sha>` rather than trusted off the command line.
#
# ci/bench-attrib-through.sh takes its base/tip directories and shas as plain
# arguments and, until this script existed, never checked that the directory
# actually held that commit's source — a stale directory (yesterday's tip
# rebuilt, a leftover from a different item, a `git archive` that grabbed the
# wrong ref) would carry whatever sha the caller typed and produce a silent
# wrong attribution number. Same shape as the mtime hazard in
# `docs/AGENT-BRIEF.md`: every trace agrees with every other trace and all of
# them are wrong. This is the check that can go red for it.
#
#     ci/bench-stamp-sha.sh <dir-under-HOME> <sha> [repo]
#
# Compares dir/{crates,tools,Cargo.toml} against `git archive <sha>` taken
# from `repo` (default: this script's own repo checkout). Refuses (exit 3)
# and writes nothing on any difference. On a clean match, writes
# <dir>/.bench-sha = <sha>, which ci/bench-attrib-through.sh then requires
# and checks before running any arm.
#
# Cargo.lock is deliberately NOT compared. It is a derived artifact, not
# source: this box's local-path `grit-plugin-api` dependency resolves to
# whatever version sits on disk right now regardless of which commit is
# built (it is not fetched from a registry, so nothing pins it), and a
# commit's own checked-in lock can drift out of sync with its Cargo.toml
# between commits without anyone running `cargo update` — `cargo build`
# repairs that silently. Checked directly: `9b4afd4e`'s committed Cargo.lock
# disagrees with the freshly-built `rustmc-benchB` lock (an extra
# `anvil-probe` package entry, one added `serde` edge) while every file
# under `crates/`, `tools/` and `Cargo.toml` is byte-identical — so a
# Cargo.lock diff here would refuse a directory that is, for every path
# `worldgen-dump` actually depends on, the right source.
set -euo pipefail
DIR="${1:?usage: bench-stamp-sha.sh <dir-under-HOME> <sha> [repo]}"
SHA="${2:?usage: bench-stamp-sha.sh <dir-under-HOME> <sha> [repo]}"
REPO="${3:-$(cd "$(dirname "$0")/.." && pwd)}"
TARGET="$HOME/$DIR"

[ -d "$TARGET" ] || { echo "bench-stamp-sha: no such dir $TARGET"; exit 2; }
case "$SHA" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*) ;;
    *) echo "bench-stamp-sha: sha does not look like hex: $SHA"; exit 2 ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git -C "$REPO" archive "$SHA" -- crates tools Cargo.toml 2>/dev/null | tar -x -C "$TMP" \
    || { echo "bench-stamp-sha: cannot archive $SHA from $REPO — bad sha or not a git checkout"; exit 2; }

fails=0
for p in crates tools Cargo.toml; do
    [ -e "$TMP/$p" ] || continue
    if ! diff -rq "$TMP/$p" "$TARGET/$p" 2>&1; then
        fails=1
    fi
done

if [ "$fails" -ne 0 ]; then
    echo "bench-stamp-sha: REFUSED — $TARGET does not match $SHA, no stamp written"
    exit 3
fi

echo "$SHA" > "$TARGET/.bench-sha"
echo "bench-stamp-sha: verified clean against $REPO, stamped $TARGET/.bench-sha = $SHA"
