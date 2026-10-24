#!/usr/bin/env bash
# box-env.sh — the shell half of ci/lib/box_env.py.
#
# Source it, then call `box_skip <check> <reason...>`. It prints the same
# `BOX-SKIP` line the python module prints and appends it to
# `$RUSTMC_BOX_SKIP_LOG` when that is set, so `ci/box-skip-audit.py` sees a
# shell stage's skip on the same terms as a python one.
#
# The append is what makes this worth a file rather than an `echo`. Thirteen
# stages in `ci/t0.sh` filter their output — four through a fixed `grep -E`
# pattern list, nine through `tail -1` or `tail -2` — so a marker printed on
# stdout by a stage in the middle of its run reaches nobody. That is how the
# first version of this repair produced a skip that was correct, printed, and
# absent from every gate log.
box_skip() {
    local check="$1"; shift
    local line="BOX-SKIP  $check  $*"
    printf '%s\n' "$line"
    if [ -n "${RUSTMC_BOX_SKIP_LOG:-}" ]; then
        printf '%s\n' "$line" >> "$RUSTMC_BOX_SKIP_LOG" 2>/dev/null || true
    fi
}
