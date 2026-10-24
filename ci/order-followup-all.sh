#!/usr/bin/env bash
set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"
./ci/order-artefact-report.sh
./ci/order-stage-attribution.sh
./ci/order-zx-adjudicate.sh
echo "ORDER FOLLOWUP ALL DONE"
