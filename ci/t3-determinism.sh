#!/usr/bin/env bash
# G4 — determinism double-run (ORACLE.md §6.3): two identical-seed full
# generations, byte-compared, under load. Standing merge-train check.
# Live from the first moment `rustmc genregions` produces output (wave 2).
set -euo pipefail
SEED="${RUSTMC_CANON_SEED:-20260708}"   # canonical seed is config, not folklore
RADIUS="${1:-3}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT
BIN="$(dirname "$0")/../target/release/rustmc-server"
"$BIN" genregions --seed "$SEED" --radius "$RADIUS" --center 0 0 --dimension overworld --out "$OUT/a"
"$BIN" genregions --seed "$SEED" --radius "$RADIUS" --center 0 0 --dimension overworld --out "$OUT/b"
diff -r "$OUT/a" "$OUT/b" >/dev/null || { echo "G4 RED: nondeterministic output — STOP THE LINE"; exit 1; }
echo "G4 GREEN (seed $SEED r$RADIUS byte-identical)"
