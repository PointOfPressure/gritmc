#!/usr/bin/env bash
# T2 — the gates that need a REAL VANILLA SERVER running (CI.md).
#
# This tier used to be a stub that reported SKIP while waiting on an adapter
# from another project. The from-scratch amendment removed that dependency —
# every oracle in this repo is first-party now — and the stub outlived it by
# long enough to be worth saying plainly: **a tier that always reports SKIP
# is indistinguishable from a tier that passes, and neither tells you
# anything.**
#
# What runs here, both against a real vanilla server booted from the pinned
# jar:
#
#   ci/t2-wire.sh       status, login -> configuration -> play, and a
#                       scripted check round-trip. The proof that the wire
#                       harness talks to vanilla at all, and the thing that
#                       fails first if a packet layout has moved.
#   ci/t2-gameplay.sh   the gameplay corpus: every scenario run against
#                       vanilla and against rustmc and diffed, with the
#                       ratchet in ci/checks/gameplay-baseline.toml. See
#                       docs/SERVER.md, "The gameplay oracle".
#
# Deliberately NOT here: `ci/t2-regiondiff.sh`. It generates and diffs whole
# regions and costs minutes of CPU; it is the worldgen track's gate rather
# than one anything routinely waits on. Run it directly.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

"$ROOT/ci/t2-wire.sh"
"$ROOT/ci/t2-gameplay.sh"

echo "T2 GREEN"
