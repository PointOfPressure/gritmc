#!/usr/bin/env bash
# T2-wire — the wire harness proven end-to-end against REAL vanilla:
# status ping, full login→config→play join, and a scripted check round-trip.
# This is G2's skeleton; rustmc's binary replaces the vanilla target later.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${T2_PORT:-25698}"
DIR="$ROOT/oracle-work/t2-wire"

cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$DIR" >/dev/null 2>&1 || true; }
trap cleanup EXIT

rm -rf "$DIR"
"$ROOT/ci/vanilla-server.sh" start "$DIR" "$PORT" "minecraft:flat"

cargo build -q -p oracle-runner
BIN="$ROOT/target/debug/oracle-runner"

"$BIN" status "127.0.0.1:$PORT"
"$BIN" join "127.0.0.1:$PORT" oracle --stay-secs 5
"$BIN" check "127.0.0.1:$PORT" oracle "$ROOT/ci/checks/smoke.toml"

echo "T2-WIRE GREEN"
