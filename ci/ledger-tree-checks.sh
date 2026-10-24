#!/usr/bin/env bash
# The tree-only subset of ci/t0.sh: every check that needs no cargo, no JVM and
# no box. Run by the docs-and-figures ledger work to gate a docs/checks commit
# without occupying the Threadripper. Not a substitute for ci/t0-remote.sh.
set -u
cd "$(dirname "$0")/.." || exit 2
rc=0
run() { printf '%-42s' "$1"; shift; if "$@" >/tmp/ltc.$$.log 2>&1; then echo GREEN; else
  echo "RED"; sed -n '1,25p' /tmp/ltc.$$.log; rc=1; fi; }
run "check-figures.py"            python3 ci/check-figures.py
run "check-figures-falsify.py"    python3 ci/check-figures-falsify.py
run "check-doc-claims.py"         python3 ci/check-doc-claims.py
run "check-conflict-markers.py"   python3 ci/check-conflict-markers.py
run "setup-merge-drivers.sh"      ./ci/setup-merge-drivers.sh
run "merge-appendonly-falsify.sh" ./ci/merge-appendonly-falsify.sh
rm -f /tmp/ltc.$$.log
[ $rc -eq 0 ] && echo "LEDGER-TREE-CHECKS GREEN" || echo "LEDGER-TREE-CHECKS RED"
exit $rc
