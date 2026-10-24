#!/usr/bin/env bash
# bench-arms-setup.sh — build the arms of the ~10% CPU-s/chunk attribution.
#
#   A  ed0a29ec  2026-08-03 03:52  the tree the published BENCH.md figures
#                                  were measured on (0.082364 at r=7)
#   B  9b4afd4e  2026-08-06 14:43  the tree the +11.1% was re-measured on
#   C  5b0d5aed  2026-08-06 10:36  immediately before the trial chambers
#   D  d2f7c6e5  2026-08-06 11:06  the trial chambers, written
#   T  <branch>                    today's tree, for the re-take
#
# A vs B reproduces the published drift with both arms on one box at one
# time; C vs D attributes it. Each arm gets its OWN remote dir, so no rsync
# ever moves mtimes backwards inside one of them (AGENT-BRIEF 2026-08-08) —
# and every arm is given the SAME `data/vanilla`, copied from ~/rustmc,
# because the worldgen inputs there are version-pinned jar extracts rather
# than branch-generated tables. Sharing them is what makes this a code-only
# comparison.
set -uo pipefail

[ -d "$HOME/plugins" ] || { echo "FATAL: ~/plugins sibling missing"; exit 2; }

setup_arm() {           # setup_arm <src tree> <remote dir name>
    local src="$1" dir="$2"
    echo "=== $dir <- $src ==="
    rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
        --exclude 'data/vanilla' "$src/" "$HOME/$dir/" || return 1
    mkdir -p "$HOME/$dir/oracle-work/gameplay" \
             "$HOME/$dir/oracle-work/gameplay-selfcheck" \
             "$HOME/$dir/data/vanilla"
    local e
    for e in "$HOME"/rustmc/oracle-work/*; do
        case "$(basename "$e")" in gameplay*) continue ;; esac
        ln -sfn "$e" "$HOME/$dir/oracle-work/"
    done
    cp -rn "$HOME"/rustmc/data/vanilla/. "$HOME/$dir/data/vanilla/" 2>/dev/null
    # rsync -a preserves mtimes and cargo trusts them; an arm checked out at
    # an older commit arrives older than artefacts built minutes ago.
    ( cd "$HOME/$dir" && find crates tools ci -type f \
        \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' \) -print0 \
        | xargs -0 touch )
    ( cd "$HOME/$dir" && PATH="$HOME/.cargo/bin:$PATH" \
        cargo build --release -p worldgen-dump 2>&1 | tail -3 )
    ls -la "$HOME/$dir/target/release/worldgen-dump" || return 1
}

setup_arm /home/sayer/grit/work-benchattrib-A rustmc-benchA
setup_arm /home/sayer/grit/work-benchattrib-B rustmc-benchB
setup_arm /home/sayer/grit/work-benchattrib-C rustmc-benchC
setup_arm /home/sayer/grit/work-benchattrib-D rustmc-benchD
setup_arm /home/sayer/grit/work-benchattrib  rustmc-bench
echo "SETUP DONE"
