#!/usr/bin/env bash
# corpus-ports.sh — free ports for a corpus harness that may now run beside another.
#
#     . "$ROOT/ci/corpus-ports.sh"
#     read -r VAN_PORT OUR_PORT < <(corpus_free_ports 2 subset) || exit 4
#
# # Why this exists (2026-09-24, branch `corpus-parallel`)
#
# `ci/corpus-lock.sh` became a pool: two corpus runs may hold the box at once.
# `ci/t2-gameplay.sh` has derived its ports from its own tree path since
# 2026-08-06, so two of IT never collide. The other lock takers did not:
# `ci/t2-gameplay-subset.sh` bound 25896/25897, `ci/t2-gameplay-survival.sh`
# 25898/25899, `ci/t2-gameplay-selfcheck.sh` 25694 up, and
# `ci/corpus-positive-control.sh` 27914 — the same pair for every tree on the box.
# Under a mutex that was harmless, because the lock was the thing keeping two of
# them apart. Under a pool it is the 2026-08-06 collision again: the second
# vanilla refuses to bind (exit 3, loudly, since `ci/vanilla-server.sh` probes),
# or worse, a recorder drives somebody else's server.
#
# So the default is derived, like `t2-gameplay.sh`'s: a base from the tree path
# and the caller's salt, in 28000-28999 where nothing in `ci/` binds, then probed
# and stepped until `n` ports are actually free. An explicit port variable in the
# caller still wins, exactly as before.
#
# The probe is not a reservation — something can take a port between the probe
# and the bind — and `ci/vanilla-server.sh` still refuses a busy port with exit 3.
# This makes a collision rare; that makes it legible.

# corpus_free_ports <n> [salt] — n free ports, one per line; exit 1 if it cannot.
corpus_free_ports() {
    python3 - "${ROOT:-$PWD}" "${2:-}" "$1" <<'PY'
import hashlib, socket, sys
root, salt, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
base = 28000 + (int(hashlib.md5(f"{root}\0{salt}".encode()).hexdigest()[:8], 16) % 900)
def free(p):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind(("0.0.0.0", p))
        return True
    except OSError:
        return False
    finally:
        s.close()
got = []
p = base
while len(got) < n and p <= 28999:
    if free(p):
        got.append(p)
    p += 1
if len(got) < n:
    sys.exit(1)
print("\n".join(map(str, got)))
PY
}
