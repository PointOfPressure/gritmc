#!/usr/bin/env bash
# order-artefact-all.sh — references, dumps, diffs and report in one job, so no
# `ci/t0-remote.sh --sync` from the laptop can land in the middle of it.
set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"
./ci/order-vanilla-refs.sh
./ci/order-artefact-test.sh
./ci/order-artefact-report.sh
echo "ORDER ARTEFACT ALL DONE"
