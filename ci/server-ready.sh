#!/usr/bin/env bash
# server-ready.sh — the arms of a readiness proof that observe rather than grep.
#
# Sourced, not run:
#
#     . "$ROOT/ci/server-ready.sh"
#
# # Why this file exists
#
# Three harness scripts proved this server was up by grepping its log for
# `listening on`, which `crates/rustmc-server/src/main.rs` printed *before*
# calling `status::listen` and therefore before the bind. A server whose bind
# was refused printed the readiness line and then `listen: Address already in
# use (os error 98)`, in that order, and the wait loop broke out on the first
# of the two. Everything measured afterwards was measured against a port this
# process does not hold — in the failure that names itself, against whichever
# server already had it. `ci/liveness-proofs.py` found all three and pinned
# them; `docs/PARITY-ROADMAP.md` §28 is the repair.
#
# The server half of that repair is that the line is now printed by
# `status::listen`, past the bind, from immediately before the first `accept`,
# carrying `listener.local_addr()` — the address the kernel gave, not the one
# that was asked for. That makes the grep sound, and `ci/readiness-falsify.sh`
# is the evidence, taken against the real binary in both directions.
#
# But a log line is still the subject's own account of itself, and two things
# it cannot settle survive the repair:
#
#   * **whose socket is on that port.** Our bind failing and somebody else's
#     server answering are the same situation from outside, and a proof that
#     reads only our log distinguishes them only because our log is now
#     missing a line. `ready_socket_owner` asks the kernel instead: is the
#     listening socket on this port held by *this pid*.
#   * **whether it answers.** A bound socket with a wedged accept loop takes
#     connections into the backlog and serves none of them.
#     `ready_speaks_status` runs the recorder's own client stack against it,
#     so what it proves is the thing the next line of the script is about to
#     rely on.
#
# # An instrument that is not there must refuse, not report "not ready"
#
# Both functions `exit 3` when their tool is missing rather than returning
# false. A missing `ss` returning "not ready" would put the caller into a
# sixty-second wait and then a timeout blaming the server, which is the same
# class of defect this whole file is about: a check that stopped being able to
# tell, reporting the answer it can still produce.

# ready_socket_owner <port> <pid> — true when the LISTEN socket on <port>
# belongs to <pid>.
#
# `ss -ltnpH 'sport = :N'`, whose `users:(("rustmc-server",pid=N,fd=M))` field
# is readable without root for a process of our own user. The trailing comma
# in the match is not decoration: `pid=123` is a prefix of `pid=1234`.
ready_socket_owner() {
    local port="$1" pid="$2" out
    [ -n "$port" ] && [ -n "$pid" ] || {
        echo "ready_socket_owner: needs <port> <pid>, got '$port' '$pid'" >&2
        exit 3
    }
    command -v ss >/dev/null 2>&1 || {
        echo "ready_socket_owner: ss(8) is not on PATH, so this readiness proof cannot
be taken. Install iproute2. Refusing rather than reporting the subject
not-ready, which would blame the server for a missing instrument." >&2
        exit 3
    }
    out="$(ss -ltnpH "sport = :$port" 2>/dev/null)" || {
        echo "ready_socket_owner: ss failed on port $port" >&2
        exit 3
    }
    printf '%s\n' "$out" | grep -c "pid=$pid," >/dev/null
}

# ready_speaks_status <oracle-runner> <host:port> — true when the server on
# <host:port> completes a status exchange.
#
# The runner's own `client::status`, which is the client stack `record` uses,
# so a pass means the recorder can talk to this server and not merely that
# something accepted a TCP connection. Bounded by `timeout` because a wedged
# server that accepts and never answers is one of the states this is here to
# catch, and hanging on it would turn a caught failure into a stuck run.
ready_speaks_status() {
    local bin="$1" hostport="$2"
    [ -x "$bin" ] || {
        echo "ready_speaks_status: no oracle-runner at '$bin' — build it before waiting
on the server, or this proof cannot be taken." >&2
        exit 3
    }
    timeout 10 "$bin" status "$hostport" >/dev/null 2>&1
}
