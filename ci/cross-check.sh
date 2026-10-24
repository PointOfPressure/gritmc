#!/usr/bin/env bash
# ===== AGENT INTERFACE =====
#
#   ./ci/cross-check.sh                         every platform the server ships on
#   ./ci/cross-check.sh --targets "T1 T2"       a subset
#   CROSS_ROOT=/some/copy ./ci/cross-check.sh   a different workspace (the falsifier's)
#
# Exit 0 every target type-checks | 1 a target does not | 2 a target could not
# be installed (a refusal: nothing was checked for it).
# ===== end interface =====
#
# `cargo check --locked` of the `rustmc-server` binary for each platform the
# server has to build on: Windows 10+, macOS, FreeBSD, Android (API 21) and
# the host. `check` stops before codegen and linking, so no platform's C
# toolchain or SDK is needed, only the target's `rust-std`, which is
# installed with `rustup target add` when missing.
#
# What this can and cannot see. It catches every compile-time platform
# dependency in the server and everything it links: a `std::os::unix` or
# `std::os::linux` use, an `extern "C"` naming a symbol behind the wrong
# `#[cfg]`, a dependency that does not build for the target. It does NOT
# catch a runtime platform assumption that compiles everywhere, such as
# reading `/proc`; those are behind `#[cfg(target_os)]` with an explicit
# "unavailable" (see `crates/rustmc-server/src/tiles.rs`, `vm_rss_kb`).
#
# `aarch64-linux-android`'s minimum API level is a linker-time property (the
# NDK sysroot a link picks), so a `check` proves the Rust source builds for
# the target, not that it links at API 21. Nothing in the server's dependency
# tree has a build script that compiles C (`cargo tree -e normal,build
# --target all`: flate2 on miniz_oxide, serde, serde_json, and path crates),
# so there is no NDK-level API call here to pin.
#
# `ci/cross-check-falsify.sh` is its gate: an injected `std::os::unix` use is
# RED for Windows and GREEN for the host.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WS="${CROSS_ROOT:-$ROOT}"
export PATH="$HOME/.cargo/bin:$PATH"

HOST="$(rustc -vV | sed -n 's/^host: //p')"
TARGETS_DEFAULT="x86_64-pc-windows-gnu aarch64-apple-darwin x86_64-unknown-freebsd aarch64-linux-android $HOST"
TARGETS="$TARGETS_DEFAULT"
if [ "${1:-}" = "--targets" ]; then TARGETS="${2:?--targets needs a list}"; fi

installed="$(rustup target list --installed 2>/dev/null)"
RED=0 REFUSED=0 OK=0
for t in $TARGETS; do
    if ! printf '%s\n' "$installed" | grep -cx -- "$t" >/dev/null; then
        echo "cross-check: installing rust-std for $t"
        if ! timeout 900 rustup target add "$t" </dev/null >/dev/null 2>&1; then
            echo "REFUSED  $t  rustup target add failed; nothing was checked for it"
            REFUSED=$((REFUSED + 1))
            continue
        fi
    fi
    log="$(mktemp)"
    if (cd "$WS" && cargo check --locked -q -p rustmc-server --bin rustmc-server --target "$t") \
            >"$log" 2>&1 </dev/null; then
        echo "OK       $t"
        OK=$((OK + 1))
    else
        echo "RED      $t"
        grep -E '^(error|warning: unused)' -A3 "$log" | head -20 | sed 's/^/         /'
        RED=$((RED + 1))
    fi
    rm -f "$log"
done
echo "CROSS-CHECK ok=$OK red=$RED refused=$REFUSED over: $TARGETS"
[ "$RED" -eq 0 ] || exit 1
[ "$REFUSED" -eq 0 ] || exit 2
exit 0
