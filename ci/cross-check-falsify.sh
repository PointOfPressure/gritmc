#!/usr/bin/env bash
# cross-check-falsify.sh — prove ci/cross-check.sh goes red on a platform
# dependency, and only on the platform it breaks.
#
# Works on a COPY of the workspace (crates/, tools/, plugins/, the manifests
# and .cargo/), never on the tree it is run from, and shares this tree's
# target directory so registry dependencies are not rebuilt.
#
#   1  the unmutated copy is GREEN for Windows          (the copy builds)
#   2  `std::os::unix` injected into rustmc-server:
#        RED for x86_64-pc-windows-gnu, naming `unix`   (the stage bites)
#   3  the same injection is GREEN for the host         (it is the platform,
#                                                        not a broken edit)
#
# Exit 0 all three held | 1 one did not | 2 could not set up.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
HOST="$(rustc -vV | sed -n 's/^host: //p')"
WIN=x86_64-pc-windows-gnu
PLUGINS="$(cd "$ROOT/../plugins" 2>/dev/null && pwd)" \
    || { echo "cross-check-falsify: no ../plugins beside $ROOT" >&2; exit 2; }

BASE="$(mktemp -d)"
trap 'rm -rf "$BASE"' EXIT
WS="$BASE/ws"
mkdir -p "$WS"
ln -s "$PLUGINS" "$BASE/plugins"
(cd "$ROOT" && cp -a Cargo.toml Cargo.lock .cargo crates tools plugins "$WS/") \
    || { echo "cross-check-falsify: could not copy the workspace" >&2; exit 2; }
export CARGO_TARGET_DIR="$ROOT/target"

PASS=0 FAIL=0
ok()  { echo "  ok    $1"; PASS=$((PASS + 1)); }
bad() { echo "  FAIL  $1"; FAIL=$((FAIL + 1)); }

out="$(CROSS_ROOT="$WS" "$ROOT/ci/cross-check.sh" --targets "$WIN" 2>&1)"; rc=$?
[ "$rc" -eq 0 ] && ok "1 the unmutated copy is green for $WIN" \
                || { bad "1 the unmutated copy is not green for $WIN (rc=$rc)"; printf '%s\n' "$out" | head -20; }

F="$WS/crates/rustmc-server/src/tiles.rs"
cat >> "$F" <<'EOF'

/// Injected by ci/cross-check-falsify.sh: a Unix-only API in shipped code.
#[allow(dead_code)]
fn cross_check_falsify_unix_only(p: &std::path::Path) -> usize {
    std::os::unix::ffi::OsStrExt::as_bytes(p.as_os_str()).len()
}
EOF

out="$(CROSS_ROOT="$WS" "$ROOT/ci/cross-check.sh" --targets "$WIN" 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s\n' "$out" | grep -c "RED      $WIN" >/dev/null \
        && printf '%s\n' "$out" | grep -c 'unix' >/dev/null; then
    ok "2 an injected std::os::unix use is RED for $WIN, and the error names it"
else
    bad "2 an injected std::os::unix use was not refused for $WIN (rc=$rc)"
    printf '%s\n' "$out" | head -20
fi

out="$(CROSS_ROOT="$WS" "$ROOT/ci/cross-check.sh" --targets "$HOST" 2>&1)"; rc=$?
[ "$rc" -eq 0 ] && ok "3 the same injection is GREEN for the host $HOST" \
                || { bad "3 the injection also broke the host build (rc=$rc), so case 2 proves nothing"; printf '%s\n' "$out" | head -20; }

echo "cross-check-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
