#!/usr/bin/env bash
# t2-gameplay.sh — the gameplay oracle, end to end.
#
# Runs the whole scenario corpus against REAL VANILLA and against rustmc,
# and diffs what each one said on the wire. Vanilla is the reference; rustmc
# is the subject; neither is compared against a fixture, because a fixture is
# a record of what somebody expected rather than of what vanilla does.
#
#     ./ci/t2-gameplay.sh
#     T2_GAMEPLAY_KEEP=1 ./ci/t2-gameplay.sh    # keep the traces and worlds
#
# # The substrate
#
# Both servers run vanilla's own **superflat** preset, in creative, with
# structures off. That is not a convenience:
#
#   * superflat makes every column identical, so a scenario written relative
#     to where the player stands is the same situation on both servers by
#     construction. On the noise world it would not be — the overworld's
#     order-invariant core is zero on two seeds, which is a very strong
#     statement about terrain and still not "identical", and a check that
#     fails because a tree grew in one world has measured nothing about
#     gameplay;
#   * creative, because this server has no survival inventory to hold a
#     block and no light data for a survival player to survive in;
#   * structures off, because a village under the check's feet is a
#     difference between two worlds rather than between two servers.
#
# # What green means
#
# Not "every check matches". Most of a young corpus is legitimately `absent`
# — the checks for mechanics nobody has built yet are the ones worth writing
# early, because they are the worklist. Green means no scenario went
# backwards against `ci/checks/gameplay-baseline.toml`, and no scenario was
# `vacuous`, `mute`, `inert` or `error`, the four verdicts that say nothing
# about the server at all.
#
# `mute` is the newest and the narrowest: the scenario's **measured step** —
# its last `[[step]]`, the action the file is named for — contributed zero
# declared observations on both sides, so the check is green whatever the
# server does there. `607` passed that way for its whole life, with a healthy
# `reference_obs: 7` earned entirely by the five rig-building clicks before
# the one it was about. A check whose silence *is* its assertion says
# `measured_step = "silent"` and is compared exactly as before.
#
# `inert` is the one worth knowing about before reading a number here. Since
# `handleUseItemOn`'s two unconditional resyncs were reproduced, **every
# scenario built on `place` has a non-empty vanilla trace by construction**,
# so `vacuous` — which only fires on a silent reference — stopped being able
# to tell a live check from a dead one. `inert` subtracts what the scenario's
# own actions force and refuses the check if nothing is left.
#
# Reproducibility is a separate question and has its own instrument:
# `ci/t2-gameplay-selfcheck.sh` records the corpus against vanilla twelve
# times and diffs every run against the first. Run it before trusting a number that rests on
# a channel nobody has re-measured — three withdrawn conclusions here were all
# a reference disagreeing with itself between runs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Ports are chosen further down, once `refuse` exists — see "the ports" below.
# They were `25696` and `25697`, hardcoded, for every one of the sixty trees on
# the box.
VAN_PORT=""
OUR_PORT=""
WORK="$ROOT/oracle-work/gameplay"
# The pinned version and vanilla's own message templates, read the way
# `ci/vanilla-server.sh` reads them so the two cannot name different jars.
#
# **BOTH ARE LAZY, AND THAT IS NOT STYLE — 2026-09-05.** The version was read
# at load time for one commit, and that killed this script BEFORE the EXIT
# trap that prints its verdict was armed. `ci/t2-verdict-falsify.sh` builds
# its two fixtures with `git archive HEAD` and falls back to copying `ci/`
# ALONE when $ROOT has no `.git` — which is every rsynced gate directory, as
# that gate's own `check-cited-audits: SKIPPED — not inside a git repository`
# line says. `crates/rustmc-proto/src/lib.rs` is then absent, `grep -oP` fails
# under `set -e`, and the run exits 2 having printed neither
# `T2-GAMEPLAY REFUSED` nor `T2-GAMEPLAY RED`. That is precisely the silent
# failure that falsifier exists to refuse, reintroduced by a line that only
# wanted a version number. Measured: exit 2, no verdict line, in a directory
# holding `ci/` and nothing else.
#
# So anything that reads outside `ci/` is read where a failure can be
# reported: inside a function, on a path that is already past the trap.
vanilla_ver() {
    local v
    v="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' \
         "$ROOT/crates/rustmc-proto/src/lib.rs" 2>/dev/null)" || v=""
    [ -n "$v" ] || {
        echo "FAIL: no GAME_VERSION in $ROOT/crates/rustmc-proto/src/lib.rs" >&2
        return 1
    }
    printf '%s' "$v"
}
# `lang_re <key> [value ...]` — an extended regex matching what a vanilla
# server writes to its log for that message, derived from the pinned jar.
# `ci/lang-message-re.py` carries the argument for deriving rather than
# writing down; the short version is that `commands.time.set` still holds
# pre-26.2 wording in the lang file while being referenced nowhere in the
# code, so a phrase recalled or grepped is a phrase for whichever version the
# author remembers.
lang_re() {
    local ver jar
    ver="$(vanilla_ver)" || return 1
    jar="$ROOT/oracle-work/vanilla-$ver/versions/$ver/server-$ver.jar"
    [ -s "$jar" ] || {
        echo "FAIL: no unpacked jar at $jar — vanilla's message templates are" >&2
        echo "      read from it rather than written down. Start vanilla once" >&2
        echo "      so the bundler unpacks it." >&2
        return 1
    }
    unzip -p "$jar" assets/minecraft/lang/en_us.json \
        | python3 "$ROOT/ci/lang-message-re.py" "$@"
}
VAN_DIR="$WORK/vanilla"
# `RUSTMC_CHECKS` points this at a SUBSET, for the inner loop only.
#
# The corpus lock is contended, a full run is ~35 minutes at ~9 s a scenario,
# and an agent iterating on the wire format of one new check does not need the
# other hundred-and-some. `docs/AGENT-BRIEF.md` already tells agents to
# "iterate on subsets, one full run at the end" and until 2026-08-13 there was
# no way to do it, so what agents actually did was run the whole corpus or
# nothing.
#
# **A subset run is not a gate and this script says so in its own verdict
# line** — see the `SUBSET` tag at the bottom. The danger is not that somebody
# runs one, it is that the log of one gets quoted as "T2-GAMEPLAY GREEN": the
# ratchet in `gameplay-baseline.toml` exists to catch a scenario going
# backwards, and a run that never executed a scenario cannot see it go
# anywhere. Every assertion below that counts scenarios reads `$CHECKS`, so a
# subset run is internally consistent and its own recording assertions still
# hold; it is only the corpus-wide claim that is void.
CHECKS="${RUSTMC_CHECKS:-$ROOT/ci/checks/gameplay}"
# Pin the world. Without it every run generates a fresh one and the spawn
# point moves with it.
GAMEPLAY_SEED="${GAMEPLAY_SEED:-20260803}"
BASELINE="$ROOT/ci/checks/gameplay-baseline.toml"

# The hold, decomposed, on every run rather than on the runs somebody was
# watching. Sourced here — before the preflight, before the lock — because
# `acquire-begin` has to be recorded by the same process that is about to
# block, and because a run that refuses in the preflight is still a run whose
# absence from the report should be explicable. `ci/hold-phases.sh` costs one
# `date` and one append per marker and can never fail this script; see its
# header for why the honesty lives in its reader instead.
. "$ROOT/ci/hold-phases.sh"
hold_phase_init "$CHECKS"

RUSTMC_PID=""
SHARD_PIDS=()
# `PREFLIGHT_OK` is not tidiness. `$VAN_DIR` lives under `oracle-work/gameplay`,
# and the whole point of the preflight below is that `oracle-work` may be
# pointing at somebody else's tree. A cleanup that ran anyway would `rm -rf`
# inside that tree on its way out of a refusal — the refusal would do the damage
# it exists to prevent. Nothing destructive happens until we know the directory
# is ours.
PREFLIGHT_OK=""
# THIS SCRIPT HAD NO STRING FOR ITS OWN FAILURE — 2026-08-18.
#
# It printed `T2-GAMEPLAY GREEN` on success and `T2-GAMEPLAY REFUSED` when it
# declined to measure, and on a genuine corpus regression it exited non-zero
# SAYING NEITHER: the comparison prints `  REGRESSED <scenario>` and `set -e`
# ends the run. `ci/handoff.sh` reads the exit status, so a gate was still
# correctly RED — but nothing downstream that reads the LOG could see it, and
# three things read the log:
#
#   * `ci/gate-rate.py` classified such a gate UNJUDGED, so T2 showed 143 GREEN
#     and 0 RED across the project's whole history — in the stage that is 66%
#     of a gate. Four logs were misread; three are real T2 failures
#     (`chain-cascade-signs` twice, `gate-skips-and-scenario-verbs`, `t2=1/0`);
#   * `docs/AGENT-BRIEF.md`'s recommended waiter matched
#     `T2-GAMEPLAY (GREEN|REFUSED)` and therefore hung forever through exactly
#     this failure — in the same block that warns "a waiter that fires only on
#     green is silent through a crash";
#   * a human reading the tail of the log saw no verdict at all.
#
# Both of those were repaired at their own end. This is the repair at the
# source, so the next reader does not need a fourth workaround.
#
# IT IS PURELY ADDITIVE AND CANNOT MOVE A VERDICT. It prints one line on an
# already-failing exit. It never runs on success (rc 0, which covers GREEN and
# `SUBSET OK`) and never on a refusal (rc 4, which already prints its own
# string), so `grep -c "^T2-GAMEPLAY GREEN"` — how `ci/handoff.sh` counts a
# pass — is untouched in every case.
_t2_sentinel() {
    case "$1" in
        0|4) return 0 ;;
    esac
    echo
    echo "T2-GAMEPLAY RED — exit $1. The corpus comparison failed; look for" \
         "'  REGRESSED' lines above, or for the stage that died." >&2
}

cleanup() {
    local _t2_rc=$?
    [ -n "$RUSTMC_PID" ] && kill "$RUSTMC_PID" 2>/dev/null || true
    # Tidy the lock's holder record if we ever took it. This is NOT what
    # releases the lock — the kernel does that when this process's fds close,
    # `kill -9` included — so its absence would cost a stale line in
    # `./ci/corpus-lock.sh status` and nothing else. Guarded because `cleanup`
    # is trapped before `ci/corpus-lock.sh` is sourced, and a refusal in
    # between must not die inside its own EXIT trap.
    # Before the release, so the marker is inside the hold it ends. On every
    # exit path this trap covers, which is every path but `kill -9`.
    hold_phase release
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
    [ -n "$PREFLIGHT_OK" ] || { _t2_sentinel "$_t2_rc"; return 0; }
    # NOT `>/dev/null`. This is the line the gate hung on for 67 minutes on
    # 2026-08-05, and it hung silently partly because its output was thrown
    # away. `stop` is bounded now, and what it says about why the server was
    # already gone is the most useful sentence in the whole run.
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" || true
    # A sharded run's recorders and servers are children of its shard
    # subshells. Killing a subshell does not kill what it is waiting on, so the
    # children go first, then the subshell, then every shard's vanilla by its
    # own pid file — the same bounded `stop` as above.
    if [ "${#SHARD_PIDS[@]}" -gt 0 ]; then
        for _p in "${SHARD_PIDS[@]}"; do
            pkill -TERM -P "$_p" 2>/dev/null || true
            kill "$_p" 2>/dev/null || true
        done
        for _d in "$WORK"/shard-*/vanilla; do
            [ -d "$_d" ] || continue
            "$ROOT/ci/vanilla-server.sh" stop "$_d" >/dev/null 2>&1 || true
            [ -d "$_d/crash-reports" ] && \
                "$ROOT/ci/vanilla-server.sh" preserve "$_d" "shard crash report still present at cleanup" || true
        done
        for _f in "$WORK"/shard-*/rustmc/server.pid; do
            [ -s "$_f" ] && kill "$(cat "$_f")" 2>/dev/null || true
        done
        [ -n "${T2_GAMEPLAY_KEEP:-}" ] || rm -rf "$WORK"/shard-*/vanilla
    fi
    # Last chance to save a crash report before the `rm -rf` below eats it.
    # `t2-gameplay.sh` wipes $VAN_DIR at the start of every run *and* here, so
    # for the whole life of this script a vanilla that crashed left nothing
    # behind at all — the one artefact that names the cause was deleted by the
    # gate that needed it.
    [ -d "$VAN_DIR/crash-reports" ] && \
        "$ROOT/ci/vanilla-server.sh" preserve "$VAN_DIR" "crash report still present at cleanup" || true
    [ -n "${T2_GAMEPLAY_KEEP:-}" ] || rm -rf "$VAN_DIR"
    _t2_sentinel "$_t2_rc"
}
trap cleanup EXIT

# --- preflight: the gate must be gating THIS tree --------------------------
#
# Everything below this comment exists because on 2026-08-03 this script
# reported **12/55 with 11 regressed** for a branch that was not broken, and
# the number was believed for two hours. The gate directory had branch-tip
# source and another tree's data:
#
#   * `oracle-work` was a **symlink to a different checkout**, so the
#     recordings, the live vanilla world and the report were all written into
#     a tree nobody was gating — where a second run could and did overwrite
#     them mid-flight;
#   * `data/vanilla/set_placed_by.json`, the table the branch was entirely
#     about, was **absent**, because `data/vanilla` is gitignored, excluded
#     from the rsync, and seeded once with `cp -rn` — no-clobber, so a table
#     invented after the seeding never arrives;
#   * `data/vanilla/use_without_item.json` was **111 KB stale**, because the
#     branch rewrote `UseGolden.java` and no-clobber kept the old output.
#
# rustmc then behaved exactly like a server missing half the branch, and every
# trace said so: `powered=false` where vanilla had `true`, one `block_update`
# where vanilla sent a `section_blocks_update` carrying both halves of a bed.
# A red that means nothing is worse than no gate, because somebody acts on it.
#
# And the old preflight was itself a check that could not fail: three extracts
# missing printed `skipping: no vanilla extract` and **`exit 0`**. No corpus
# ran and the caller saw success.
#
# So: refuse, loudly, non-zero. Never skip, never default, never infer.
. "$ROOT/ci/provenance.sh"
. "$ROOT/ci/server-ready.sh"
# Defines `refcache_lookup` / `refcache_store` and does nothing at all unless
# `RUSTMC_REFERENCE_CACHE=1`. Read its header before touching the vanilla half
# of this script: it is where the argument lives for why a reference recording
# is a function of its inputs and not of the candidate branch, and what makes
# serving a cached one safe rather than merely fast.
. "$ROOT/ci/reference-cache.sh"

refuse() { echo; echo "T2-GAMEPLAY REFUSED — $*" >&2; exit 4; }

# 0. The two servers must be on the SAME difficulty, and until 2026-09-05 they
#    were not: vanilla at `peaceful` and this server at its own default. See
#    `ci/gameplay-difficulty.sh` for the two bytecode gates that makes silent
#    (`SpawnEggItem.useOn` and `Mob.checkDespawn`) and for why the launch line
#    cannot simply pass the flag. Sets `GAMEPLAY_DIFFICULTY` and
#    `DIFFICULTY_ARGS`, and refuses the one state that cannot be matched.
. "$ROOT/ci/gameplay-difficulty.sh"
difficulty_or_die "$ROOT" || exit 4

# 1. The work directory must belong to the tree being gated. A symlink is
#    allowed only if it lands back inside $ROOT; anything else means the
#    recordings describe another checkout.
for d in oracle-work oracle-work/gameplay; do
    if [ -L "$ROOT/$d" ]; then
        tgt="$(readlink -f "$ROOT/$d" 2>/dev/null || true)"
        case "$tgt/" in
            "$ROOT"/*) ;;
            *) refuse "$d is a symlink to $tgt, which is outside $ROOT.
         The gate would record into, and read from, a tree it is not gating.
         Fix:  rm $ROOT/$d   (the symlink only — never the target)
               mkdir -p $ROOT/oracle-work/gameplay
         and symlink the individual vanilla references in, not the directory." ;;
        esac
    fi
done
mkdir -p "$WORK"
[ -d "$WORK" ] || refuse "$WORK is not a directory"

# 2. Every table the server actually opens must be present, non-empty, and
#    stamped as this tree's own. Derived from the source, so a branch that
#    adds a table cannot forget to add it here.
#
# CLOSED 2026-08-07, and this grep must stay byte-for-byte the one in
# `ci/gen-gameplay-tables.sh` — a preflight that checks a *different* set from
# the one the generator produces is worse than either. The reasoning, the
# measurement and the residual are written once, there.
#
# In short: the scan root was `crates/rustmc-server/src` while the claim above
# is "every table the server actually opens", and `[a-z_]+` matched no digit
# and no capital — so a table named `signal2.json`, or one opened from
# `crates/rustmc-behavior`, was not in `REQUIRED` and this refusal could not
# fire for it. Demonstrated on a constructed tree, then measured to derive the
# same 16 tables on this one either way.
mapfile -t REQUIRED < <(grep -rhoE '"data/vanilla/[A-Za-z0-9_]+\.(json|bin)"' \
                        "$ROOT/crates" "$ROOT/tools" "$ROOT/plugins" 2>/dev/null \
                        | tr -d '"' | sort -u)
[ "${#REQUIRED[@]}" -gt 0 ] || refuse "derived no required tables — the grep in this script has gone stale"
BAD=0
for t in "${REQUIRED[@]}"; do
    if why="$(prov_check "$ROOT" "$ROOT/$t")"; then continue; fi
    echo "  $t: $why" >&2
    BAD=$((BAD + 1))
done
[ "$BAD" -eq 0 ] || refuse "$BAD of ${#REQUIRED[@]} generated tables are missing, foreign or stale.
         Run:  ./ci/gen-gameplay-tables.sh
         A gate run against another tree's tables measures that tree."
echo "preflight: ${#REQUIRED[@]} tables current and stamped for $ROOT"
# From here the work directory is known to be ours, so cleanup may delete in it.
PREFLIGHT_OK=1

# --- one corpus run on this box at a time ----------------------------------
#
# Three of these ran at once on 2026-08-06 — two agents and a merge gate — and
# the damage was not a crash. A corpus run sits in `settle_ms` waits and
# vanilla boots rather than on a core, so concurrent runs do not share a busy
# CPU: they wait out each other's timers. Throughput fell to about **one
# scenario per ten minutes at load average 0.02**, and the low load read as
# "nothing is contending, so my run is wedged" — one agent spent its turn
# hunting a stalled process that did not exist. One run was killed at 74/86,
# one at 50/86, one abandoned at 70/86.
#
# The acquire sits **here**, and the position is the design:
#
#   * **after the preflight**, so a tree with foreign or stale tables is
#     refused in seconds instead of refusing an hour later, having held the
#     box for the whole of it. Nothing above this line boots a server or
#     measures anything;
#   * **before the ports are picked**, so the probe in `pick_port_pair` runs
#     when it is already true that no other corpus is running. Probing an hour
#     before you bind is how you get the collision the probe exists to avoid;
#   * **in the harness, not in its callers.** `ci/handoff.sh` stage 6 runs this
#     script and so does every agent, so one acquire site covers both and
#     neither has to remember. A rule that lives in the caller is the rule that
#     just failed.
#
# It waits; it does not abort. An abort would move the problem into the
# caller's error handling and throw away a preflight already paid for.
# `RUSTMC_CORPUS_LOCK_ROLE=gate` makes a waiting run hand the lock back so the
# gate goes first. **There IS a queue as of 2026-08-12, and this line used to
# say there was not** — runs are served oldest-first among themselves, and the
# hand-back is still what puts a gate in front of all of them. Nothing here
# changes: it is the same one `corpus_lock_acquire` call, and it still waits
# rather than aborting.
# --- the release build, and it is DELIBERATELY OUTSIDE THE LOCK ------------
#
# It used to sit forty lines below, between the port pick and the vanilla boot,
# and therefore inside the hold. Moving it up is the whole of this change and
# the argument for it is short: **a release build boots no server, binds no
# port, and reads nothing that another corpus run can be writing.** Its inputs
# are this tree's `crates/`, `tools/` and `target/`, none of which are shared —
# `ci/handoff.sh` gives every lane its own remote directory, and the preflight
# above has already refused a tree whose `oracle-work` is somebody else's.
#
# NOTHING BETWEEN THE OLD SITE AND HERE WAS A PREREQUISITE. The port pick is
# the only thing that was, and it neither feeds the build nor is fed by it;
# `BIN` is not read until the reference recording, far below.
#
# **The three properties the acquire's position is documented for all survive,
# and they are the reason this is a move rather than a deletion:**
#
#   * *after the preflight* — still true; the preflight is above both, and a
#     tree with foreign or stale tables is still refused in seconds. The build
#     now sits between the preflight and the acquire, which is the one gap
#     where work is free: it is not holding the box, and the tree has already
#     been proved to be its own;
#   * *before the ports are picked* — still true, and now more tightly. The
#     acquire is immediately followed by `pick_port_pair` with nothing in
#     between, where a `cargo build` used to sit between the probe and the
#     bind. That gap is the probe's one acknowledged weakness — "something can
#     take the port between the probe and the bind" — and this shortens it
#     from a whole build to nothing;
#   * *in the harness, not in its callers* — still true. One
#     `corpus_lock_acquire`, in this file, covering `ci/handoff.sh` stage 6 and
#     every agent alike. `RUSTMC_CORPUS_LOCK_ROLE=gate` and the hand-back are
#     untouched: this file never mentioned them and still does not.
#
# **AND IT IS WORTH FAR LESS THAN THE BRIEF THAT ASKED FOR IT EXPECTED.**
# Measured rather than assumed, over 20 builds in a gate-shaped tree
# (`docs/CORPUS-LOCK-HOLD.md` §6): a warm no-op is 0.17 s, the realistic
# one-crate-changed arms are 30-47 s, and a *cold* build of the whole
# workspace — which bounds every incremental build above, because it does
# strictly more work — is 61.9 s. So this returns about **1 minute of a
# ~44-minute hold, 2.3%**. It is made anyway: it is free, it shortens the
# probe-to-bind gap, and a build that fails now fails without having taken the
# box. It is emphatically **not** where the lock's time goes. That is the
# reference recording, and `ci/reference-cache.sh` is the only thing that
# touches it.
hold_phase build-begin
cargo build -q -p oracle-runner --release
cargo build -q -p rustmc-server --release
BIN="$ROOT/target/release/oracle-runner"
hold_phase build-done

. "$ROOT/ci/corpus-lock.sh"
hold_phase acquire-begin
corpus_lock_acquire
hold_phase acquire-end

# --- the ports --------------------------------------------------------------
#
# Until 2026-08-06 this file hardcoded 25696 and 25697 **for every tree on the
# box**, and there are sixty. Two gates, or a gate and an agent's own corpus
# run, race for the same two ports. The loser's vanilla cannot bind, throws
# `IllegalStateException: Failed to initialize server`, and writes a crash
# report — and `ci/vanilla-server.sh` used to report that as five lines of tail
# that are *identical for every server-thread exception*, so the collision read
# as a mystery crash in our own tree. It cost two gate runs on 2026-08-05.
#
# Two changes, and the second one is the one that makes it safe rather than
# merely unlikely:
#
#   * an explicit override always wins. Other scripts and other agents set
#     these variables, and renumbering under a live concurrent run would be the
#     same class of bug in the other direction;
#   * with no override, the base is derived from the tree's own path, so two
#     trees never *start* from the same pair, and then the pair is probed and
#     stepped until it is actually free. Deterministic where it can be, checked
#     where it cannot.
#
# 26xxx because nothing else in `ci/` is there: the selfcheck walks 25694-25705
# (which straddles the old defaults), `t2-gameplay-subset.sh` takes 25896/25897,
# `bench-servers.sh` 25801+, and the boot benches 25911-25915.
#
# The probe is not a proof — something can take the port between the probe and
# the bind — so `ci/vanilla-server.sh` still refuses on a busy port and still
# names a bind failure explicitly. This makes collisions rare; that makes them
# legible when they happen anyway.
pick_port_pair() {   # pick_port_pair [count] — one "van our" line per pair
    python3 - "$ROOT" "${1:-1}" <<'PY'
import hashlib, socket, sys
root = sys.argv[1]
base = 26000 + 2 * (int(hashlib.md5(root.encode()).hexdigest()[:8], 16) % 400)
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
want = int(sys.argv[2]) if len(sys.argv) > 2 else 1
got = 0
for i in range(100 + 2 * want):
    p = base + 2 * i
    if p + 1 > 26999:
        break
    if free(p) and free(p + 1):
        print(p, p + 1)
        got += 1
        if got == want:
            sys.exit(0)
sys.exit(1)
PY
}
# How many isolated pairs this run records on — see "the shards" further down.
# 8 by default since 2026-10-01: `ci/checks/corpus-shard-equivalence.toml` run
# `full-shard8c-silent084-3serial-20261001` is EQUIVALENT against two serial
# controls over all 179 scenarios, and `ci/corpus-shard-equivalence.py --check`
# (T0) refuses any default the ledger does not prove. Lock hold for the full
# corpus: ~5,000 s serial, ~516 s at 8. `RUSTMC_CORPUS_SHARDS=1` is serial.
SHARDS="${RUSTMC_CORPUS_SHARDS:-8}"
case "$SHARDS" in
    ''|*[!0-9]*) refuse "RUSTMC_CORPUS_SHARDS='$SHARDS' is not a positive integer" ;;
esac
[ "$SHARDS" -ge 1 ] && [ "$SHARDS" -le 16 ] || refuse "RUSTMC_CORPUS_SHARDS=$SHARDS is outside 1..16"
_nc=$(find "$CHECKS" -maxdepth 1 -name '*.toml' | wc -l)
[ "$_nc" -ge 1 ] && [ "$SHARDS" -gt "$_nc" ] && SHARDS="$_nc"
# Exported as the value actually used, because `ci/reference-cache.sh` keys a
# reference on it and must see this number, not whatever the caller passed.
export RUSTMC_CORPUS_SHARDS="$SHARDS"
if [ -n "${T2_GAMEPLAY_VANILLA_PORT:-}" ] || [ -n "${T2_GAMEPLAY_RUSTMC_PORT:-}" ]; then
    # Setting one and not the other must not silently reinstate the old shared
    # default for the partner — that is the collision this block exists to end,
    # and it would be reintroduced by a caller doing something reasonable.
    # The partner is derived from whichever was given.
    VAN_PORT="${T2_GAMEPLAY_VANILLA_PORT:-}"
    OUR_PORT="${T2_GAMEPLAY_RUSTMC_PORT:-}"
    [ -n "$VAN_PORT" ] || VAN_PORT=$((OUR_PORT - 1))
    [ -n "$OUR_PORT" ] || OUR_PORT=$((VAN_PORT + 1))
    echo "ports: :$VAN_PORT vanilla, :$OUR_PORT rustmc (explicit override)"
else
    read -r VAN_PORT OUR_PORT < <(pick_port_pair) || true
    [ -n "${VAN_PORT:-}" ] && [ -n "${OUR_PORT:-}" ] || refuse \
        "could not find a free port pair in 26000-26999 for $ROOT.
         Something is holding an implausible number of ports, or python3 is broken.
         Override with T2_GAMEPLAY_VANILLA_PORT / T2_GAMEPLAY_RUSTMC_PORT."
    echo "ports: :$VAN_PORT vanilla, :$OUR_PORT rustmc (derived from $ROOT, probed free)"
fi
# One pair per shard. Shard 0 is the pair above, so an unsharded run binds
# exactly what it always did; the rest are probed the same way, from the pair
# after it. An explicit override pins shard 0 and steps by two from there.
SHARD_PAIRS=("$VAN_PORT $OUR_PORT")
if [ "$SHARDS" -gt 1 ]; then
    if [ -n "${T2_GAMEPLAY_VANILLA_PORT:-}" ] || [ -n "${T2_GAMEPLAY_RUSTMC_PORT:-}" ]; then
        for i in $(seq 1 $((SHARDS - 1))); do SHARD_PAIRS+=("$((VAN_PORT + 2 * i)) $((OUR_PORT + 2 * i))"); done
    else
        mapfile -t SHARD_PAIRS < <(pick_port_pair "$SHARDS") || true
        [ "${#SHARD_PAIRS[@]}" -eq "$SHARDS" ] || refuse \
            "found ${#SHARD_PAIRS[@]} free port pairs in 26000-26999 for $SHARDS shards."
        read -r VAN_PORT OUR_PORT <<<"${SHARD_PAIRS[0]}"
    fi
    echo "ports: $SHARDS shard pairs: ${SHARD_PAIRS[*]}"
fi

hold_phase ports-done

# --- vanilla, the reference ------------------------------------------------
#
# **This half of the gate does not depend on the branch being gated, and it is
# half of the lock hold.** Measured 2026-08-16 over five gate trees by phase
# mtime and corroborated by twenty-six cold selfcheck boots: the reference
# recording costs 12.09 s per scenario against the subject's 12.21 s — 21.3 of
# the ~43 in-lock minutes at N=106. Nothing our server produces reaches vanilla;
# `record` drives the corpus at `127.0.0.1:$VAN_PORT` and writes down what came
# back. `docs/CORPUS-LOCK-HOLD.md` has the method and the numbers.
#
# So if every input to that recording is unchanged, re-deriving it is 21 minutes
# of the box's most contended resource spent reproducing a byte-identical
# artefact. `ci/reference-cache.sh` keys it on the content of those inputs — the
# jar, the corpus, the recorder, the projection, the substrate, the three tables
# the harness sends item and state ids from, the seed — and serves it if and
# only if every one of them matches.
#
# **On a hit no vanilla boots at all.** That is the saving and it is also the
# risk, so read `ci/reference-cache-falsify.sh` before trusting it: twenty cases
# asserting that each mutated input misses *and names itself*, that a damaged
# entry is refused rather than served, and that the 2026-08-03 shared-tables
# defect is caught. Everything downstream still runs unchanged — the served
# recording goes through `assert_recording` (which refuses a denominator that is
# not this corpus) and `recording_still_ours` exactly as a fresh one does.
#
# **The flag's arrival is printed before anything reads it, and that is not
# decoration.** This script is invoked by `ci/handoff.sh` through an `ssh`
# command string, and no variable exported in the calling shell crosses that
# boundary — see the block above stage 6 there. A flag that was dropped on the
# way in presents downstream as a cache that never hits, and a cache that never
# hits is indistinguishable from a correct cache that is legitimately missing
# every time. One line naming the value THIS process received, on THIS box,
# separates the two before either can be mistaken for the other.
echo "reference-cache: RUSTMC_REFERENCE_CACHE=${RUSTMC_REFERENCE_CACHE:-<unset>} received in $ROOT on $(hostname -s)"
REF_CACHED=""
if refcache_lookup "$ROOT" "$BIN" "$GAMEPLAY_SEED" "$WORK/vanilla.json"; then
    REF_CACHED=1
    hold_phase refcache-hit
else
    hold_phase refcache-miss
fi

# Freeze the day so nothing in the corpus depends on the sun, and make the
# world quiet: a mob wandering into a workspace is an observation nobody
# asked for and would differ run to run.
# **The gamerule ids are 26.2's, and the old ones were silently doing
# nothing.** Game rules became a registry in 26.2 — `minecraft:game_rule`, 59
# entries — with snake_case namespaced ids. `gamerule randomTickSpeed 0`
# answers "Incorrect argument for command" and carries on, so from the day
# this corpus was written until 2026-08-03 **none of these four was ever
# applied**, and this file and docs/SERVER.md both asserted they were. Random
# ticks were running the whole time, which is what turned the grass under
# every rig block into dirt at unpredictable moments and made checks flap.
#
#   doDaylightCycle -> advance_time        doMobSpawning  -> spawn_mobs
#   doWeatherCycle  -> advance_weather     randomTickSpeed -> random_tick_speed
#
# `gamerule_or_die` is the other half of the fix: a console command that
# fails must not be able to fail quietly. A substrate this file claims and
# does not have is worse than one it never claimed.
# **A console answer is waited for, not assumed after a second.** Both
# helpers below used to `sleep 1` and then grep once. Vanilla drains its console
# in `MinecraftServer.handleConsoleInputs`, once per tick, and on a loaded box
# its first ticks after `Done (` are slow: on structure-mobs' gate of
# 2026-10-04 01:07 (gate-structure-mobs-20261003-230031.log:1545-1579) three
# of eight reference shards refused with `never confirmed 'gamerule
# advance_time false'` one second after `Done (2.338s)!`, their logs holding
# neither the confirmation nor a refusal — the line had not been read yet.
#
# So this polls the log for EITHER verdict until one appears or
# `CONSOLE_CONFIRM_S` (default 60) passes: 0 confirmed, 1 refused, 2 neither
# in time. The refusal is still read first-class, so a rejected command fails
# as fast as it did, and the predicate is still the jar-derived confirmation:
# a command that never takes effect still refuses, after the deadline instead
# of after a second. Registered in `ci/checks/liveness-proofs.toml`.
CONSOLE_CONFIRM_S="${CONSOLE_CONFIRM_S:-60}"
console_confirm() {   # console_confirm <vanilla dir> <confirmation ERE>
    local dir="$1" re="$2" i
    for i in $(seq 1 "$((CONSOLE_CONFIRM_S * 4))"); do
        grep -qE "$re" "$dir/logs/latest.log" 2>/dev/null && return 0
        grep -qE "Incorrect argument|Unknown or incomplete" "$dir/logs/latest.log" 2>/dev/null && return 1
        sleep 0.25
    done
    return 2
}
gamerule_or_die() {
    local dir="$1" rule="$2" value="$3"
    # The console call's own exit status is read now. It was not, and that is
    # the same defect this function exists to fix, one level down: a command
    # whose failure is not read is not a command. `console` refuses on a dead
    # server rather than blocking on the fifo, so it can now fail *fast* — and
    # a silently dropped gamerule would put us straight back to random ticks
    # running through every measurement.
    "$ROOT/ci/vanilla-server.sh" console "$dir" "gamerule $rule $value" || {
        echo "FAIL: could not deliver 'gamerule $rule $value' to vanilla in $dir" >&2
        exit 1
    }
    # THE POSITIVE HALF, ADDED 2026-09-05 — this helper had only the negative
    # one, and `time_set_or_die` below says in its own comment why that is not
    # enough: a command that is *accepted* and does nothing leaves the
    # substrate wherever it was and every measurement then describes a world
    # nobody asked for. The 2026-08-03 incident this file is built around was
    # caught by the refusal text; a rule that were silently retired to a no-op
    # would print no refusal at all and this helper would have passed it.
    #
    # Derived from the pinned jar by key, not written down, for the reason
    # `time_set_or_die` gives at length: `commands.time.set` is proof that a
    # dead key keeps its old wording in the lang file long after the code has
    # stopped using it.
    local gr_re
    gr_re="$(lang_re commands.gamerule.set "$rule" "$value")" || {
        echo "FAIL: could not derive the 'gamerule' confirmation from the pinned jar." >&2
        echo "      commands.gamerule.set is absent or no longer takes two arguments." >&2
        exit 1
    }
    local rc=0
    console_confirm "$dir" "$gr_re" || rc=$?
    if [ "$rc" -eq 1 ]; then
        echo "FAIL: vanilla refused 'gamerule $rule $value' — see $dir/logs/latest.log" >&2
        grep -E "Incorrect argument|Unknown or incomplete" -A1 "$dir/logs/latest.log" | tail -4 >&2
        exit 1
    elif [ "$rc" -ne 0 ]; then
        echo "FAIL: vanilla never confirmed 'gamerule $rule $value' in ${CONSOLE_CONFIRM_S}s — see $dir/logs/latest.log" >&2
        echo "      the derived predicate was: $gr_re" >&2
        tail -5 "$dir/logs/latest.log" >&2
        exit 1
    fi
    # What was actually delivered, for the scenario-restore assertion below to
    # check against. Recorded here rather than listed there so that the two can
    # never disagree.
    printf '%s=%s\n' "$rule" "$value" >> "$WORK/harness-gamerules.txt"
}
# `time set <n>` on the reference. Deliberately NOT folded into
# `gamerule_or_die`: the day time is not a game rule, `time` is a different
# command with a different failure text, and a helper that took either would
# have to branch on which — which is how the snake_case defect above got in.
time_set_or_die() {
    local dir="$1" value="$2"
    "$ROOT/ci/vanilla-server.sh" console "$dir" "time set $value" || {
        echo "FAIL: could not deliver 'time set $value' to vanilla in $dir" >&2
        exit 1
    }
    # The positive half, and it is not decoration. `time set` failing silently
    # would leave the sun wherever it was and every sky check would then be
    # measuring the boot timing of two servers.
    #
    # **THIS LINE SAID `Set the time to $value` AND 26.2 DOES NOT PRINT IT.**
    # It cost `stack-worldgen` a whole T2 slot on 2026-09-05: 36 minutes queued
    # for the corpus lock, vanilla booted, and this refusal fired before a
    # single scenario ran, with vanilla's own log two lines below it reading
    # `Set minecraft:overworld to 6000 tick(s)` one second after the gamerules.
    # That is the snake_case gamerule defect this file documents twice, made a
    # third time, in the helper whose own comment says it was split out
    # *because* `time` has a different failure text.
    #
    # `commands.time.set` = `Set the time to %s` is STILL IN the 26.2 lang
    # file, which is what makes both recall and a lang grep wrong: the key is
    # dead in the code. `javap -p -c -constants
    # net/minecraft/server/commands/TimeCommand.class` references
    # `commands.time.set.absolute` twice and `commands.time.set` never.
    #
    # So the string is DERIVED FROM THE PINNED JAR rather than written down. A
    # wording change in a future jar moves this predicate with it instead of
    # turning it into fiction, and a key that has gone is a refusal rather
    # than a silent pass.
    local tmpl_re
    tmpl_re="$(lang_re commands.time.set.absolute "$value")" || {
        echo "FAIL: could not derive the 'time set' confirmation from the pinned jar." >&2
        echo "      commands.time.set.absolute is absent or is no longer" >&2
        echo "      'Set <clock> to <n> tick(s)'. Read TimeCommand again." >&2
        exit 1
    }
    local rc=0
    console_confirm "$dir" "$tmpl_re" || rc=$?
    if [ "$rc" -eq 1 ]; then
        echo "FAIL: vanilla refused 'time set $value' — see $dir/logs/latest.log" >&2
        grep -E "Incorrect argument|Unknown or incomplete" -A1 "$dir/logs/latest.log" | tail -4 >&2
        exit 1
    elif [ "$rc" -ne 0 ]; then
        echo "FAIL: vanilla never confirmed 'time set $value' in ${CONSOLE_CONFIRM_S}s — see $dir/logs/latest.log" >&2
        echo "      the derived predicate was: $tmpl_re" >&2
        tail -5 "$dir/logs/latest.log" >&2
        exit 1
    fi
    echo "substrate: vanilla day time pinned to $value"
}
# **The four rules are declared once and iterated, not listed twice.** They used
# to be four literal `gamerule_or_die` calls, which was correct while there was
# exactly one path through this code. There are two now — a run that boots
# vanilla and delivers them, and a run that serves a cached reference recorded
# on a substrate these rules already described — and a second copy of the list
# for the second path would be precisely the caveat list this file warns about
# three paragraphs up. One array, both paths, no way for them to disagree.
#
# On the cached path the rules are NOT re-delivered, because there is no server
# to deliver them to; they were delivered in the run that made the recording.
# What makes that sound rather than assumed is that this array lives in
# `ci/t2-gameplay.sh` and the cache's `harness` component is the hash of this
# file: change a rule, change the substrate, miss the cache, boot vanilla.
# The list moved to `ci/gameplay-substrate.sh` on 2026-09-05, because
# `ci/t2-gameplay-subset.sh` carried a SECOND copy of it and quieted only
# vanilla with it. The reference cache's `harness` component still hashes this
# file, and this file still sources the list, so editing it still invalidates
# the cache — see that file's header.
. "$ROOT/ci/gameplay-substrate.sh"
# **THE DAY TIME IS PINNED, AND `advance_time=false` ALONE DOES NOT PIN IT.**
#
# Added 2026-08-18, and the reason is the one this whole file is organised
# around. `advance_time=false` stops the sun; it does not agree with anything
# about *where* it stopped. Vanilla freezes wherever its day time had got to by
# the time the console command landed — a few hundred ticks after boot, and a
# different few hundred every run — and this server freezes at its own. Nothing
# noticed for as long as nothing read the sky.
#
# What it costs to leave unpinned is not a rounding error:
# `DaylightDetectorBlock.updateSignalStrength` computes
# `round(15 * cos(sunAngle))`, so the detector's `power` moves by one for
# roughly every **200 ticks** of drift between the two servers — ten seconds.
# A check on it would have been a coin toss dressed as a measurement.
#
# **6000 is noon and is chosen for a second reason.** The sun-angle track is at
# its extremum there, so `cos` is flat: 6000 +- 500 ticks all give `power = 15`.
# The pin makes the two servers agree; noon makes the agreement insensitive to
# the pin being a few ticks late on either side. One of those is the fix and
# the other is the belt.
#
# One constant, both servers. Vanilla is told through the console beside the
# game rules; this server through `--day-time`/`--advance-time` at the boot
# below, because it has no console — see `scenario::GameRule` for that gap.
SUBSTRATE_DAY_TIME=6000
# The harness-side substrate check, run once per recording of the reference.
# It lives in a function since 2026-09-24 only so that a sharded run can run
# it once per shard over that shard's own `$CHECKS`; the union is the corpus.
substrate_restore_check() {
# **A scenario's `[[gamerule]]` must restore to what those four lines just set.**
#
# A scenario may turn a rule on for its own measured window — that is the whole
# of `scenario::GameRule` and the reason `--console` is passed below. What it
# must not do is put back a *different* value, because every check after it
# would then run on a substrate nobody declared, and the failure would arrive
# as the rest of the corpus going red for a reason none of those checks names.
#
# This is a static property of the files and is asserted before anything boots
# a client, next to the four lines it is about, rather than in a validator that
# would have to keep its own copy of them. `ci/vanilla-reach-census.py` makes
# the same assertion from the other end, for the same reason.
#
# The values are read from what `gamerule_or_die` actually delivered, not from
# a second copy of the four lines above: a table of "the rules we set" written
# beside the code that sets them is this project's definition of a caveat list.
python3 - "$CHECKS" "$WORK/harness-gamerules.txt" <<'PY' || refuse "a scenario's [[gamerule]] does not restore the harness's own substrate"
import glob, os, sys, tomllib
SET = dict(l.split("=", 1) for l in open(sys.argv[2]).read().split())
if len(SET) < 4:
    print(f"  !! only {len(SET)} rules recorded by gamerule_or_die", file=sys.stderr)
    raise SystemExit(1)
bad = 0
for p in sorted(glob.glob(os.path.join(sys.argv[1], "*.toml"))):
    with open(p, "rb") as f:
        doc = tomllib.load(f)
    for g in doc.get("gamerule", ()):
        r, v, back = g.get("rule"), str(g.get("value")), str(g.get("restore"))
        if r in SET and back != SET[r]:
            print(f"  !! {os.path.basename(p)}: [[gamerule]] {r} restores to {back!r}, "
                  f"but this script sets it to {SET[r]!r}", file=sys.stderr)
            bad += 1
        elif r in SET:
            print(f"  scenario game rule: {os.path.basename(p)} -> {r}={v}, back to {back}")
raise SystemExit(1 if bad else 0)
PY
sleep 1
}

# A recording set is only a reading of this corpus if it covers this corpus.
# `~/rustmc-placedby-ticks` reported 20/**53** for the same branch that
# `~/rustmc-gate` reported 12/**55** for, because a mid-run rsync had left it
# two checks behind — two numbers that could not both be about the same thing,
# and neither of them said so. The denominator is now asserted, not printed.
N_CHECKS=$(find "$CHECKS" -maxdepth 1 -name '*.toml' | wc -l)
assert_recording() {
    local f="$1" who="$2"
    [ -s "$f" ] || refuse "$who recorded nothing — $f is missing or empty"
    local n
    n=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1])).get("scenarios",[])))' "$f") \
        || refuse "$who wrote $f but it is not readable JSON"
    [ "$n" -eq "$N_CHECKS" ] || refuse \
        "$who recorded $n scenarios but the corpus has $N_CHECKS.
         A recording that does not cover the corpus is a different denominator,
         not a worse score."
    {
        printf 'tree=%s\n'   "$ROOT"
        printf 'commit=%s\n' "$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
        printf 'checks=%s\n' "$(find "$CHECKS" -type f | sort | xargs md5sum | md5sum | cut -d' ' -f1)"
        printf 'md5=%s\n'    "$(md5sum "$f" | cut -d' ' -f1)"
        # **Which recorder made this recording.** `docs/PARITY-ROADMAP.md` §13
        # named this as the next corpus branch's errand and §15.4 as the weaker
        # half of the vocabulary question; it is both, and it is here now.
        # `ci/corpus-floor.sh` prefers this line over hashing whatever is in the
        # tree today, and the difference is the whole point: a recording made
        # here and read six branches later must carry its own decoder's identity,
        # not the decoder the reader happens to be standing next to.
        #
        # Read out of the binary that did the recording, so a tree whose sources
        # `rsync -a` left older than its artefacts stamps what actually ran.
        printf 'recorder=%s\n' "$("$BIN" recorder-id 2>/dev/null || echo unavailable)"
    } > "$f.prov"
    echo "  $who: $n/$N_CHECKS scenarios recorded, stamped for $ROOT"
}
recording_still_ours() {
    local f="$1" who="$2"
    [ -s "$f.prov" ] || refuse "$who recording has no provenance stamp"
    local t m
    t="$(sed -n 's/^tree=//p' "$f.prov")"
    m="$(sed -n 's/^md5=//p'  "$f.prov")"
    [ "$t" = "$ROOT" ] || refuse "$who recording was made in $t, not $ROOT — FOREIGN"
    [ "$m" = "$(md5sum "$f" | cut -d' ' -f1)" ] || refuse \
        "$who recording changed between being made and being compared.
         Another tree is writing into $WORK — check for a shared oracle-work."
}

# `--console` is how a scenario's own `[[gamerule]]` reaches the reference, and
# it is passed here and nowhere else.
#
# The four rules above are the substrate every check in the corpus was measured
# under, and three of them shut eight of vanilla's 120 per-tick sections
# outright (`docs/PARITY-ROADMAP.md` §21.2). A scenario that needs one of those
# systems declares the rule it needs; `record` sets it after that scenario's rig
# is quiet and puts it back before the next scenario starts, so nothing else in
# the corpus moves. See `scenario::GameRule` for why it goes through the console
# rather than through `Step::Command` — `gamerule` is permission level 2 and the
# harness's players are not operators — and for what it costs that this server
# has no console to be told through.

# `record_reference` — boot vanilla in `$VAN_DIR` on `$VAN_PORT`, put it on the
# substrate, record `$CHECKS` into `$WORK/vanilla.json`, and stop it.
#
# **A function since 2026-09-24, and its body is unchanged.** It reads only
# `$WORK`, `$VAN_DIR`, `$VAN_PORT`, `$CHECKS` and `$N_CHECKS`, so a sharded run
# (see "the shards" below) calls it in a subshell with those five pointed at
# one shard, and an unsharded run calls it in this shell exactly as the
# straight-line code used to run.
record_reference() {
echo "== booting vanilla (superflat, creative, no structures) on :$VAN_PORT"
rm -rf "$VAN_DIR"
# `difficulty=peaceful` is not tidiness, it is the substrate.
#
# Superflat is famously full of slimes — every chunk below y=40 is a slime
# chunk — and `spawn-monsters=false` plus `gamerule doMobSpawning false` did
# not stop them arriving before the gamerule took effect. Measured
# 2026-08-02: one scenario recorded **32 sound observations** that had
# nothing to do with it, and vanilla's own console answered
# `execute as @e run say` with forty-odd slimes. Peaceful removes them and
# keeps them gone, and pigs and other passive mobs are unaffected, so
# nothing a check wants to see is lost.
# `spawn-radius=0` is substrate, like `difficulty=peaceful`.
# `PlayerList.placeNewPlayer` calls `ServerPlayer.fudgeSpawnLocation`, which
# picks a random position inside the spawn radius on **every join**, so three
# consecutive runs put the reference player at (-4.5, 2.5), (-3.5, -2.5) and
# (3.5, 6.5). Anything whose bytes depend on absolute position — the block
# order inside a `section_blocks_update` does — then differs between two runs
# of the same server with nothing changed. A fixed seed pins the world; this
# pins the join.
# A failed boot must produce a *named* failure, not a stack tail and a hang.
# `start` distinguishes its failures now (3 = port collision, 2 = timeout,
# 1 = anything else) and has already preserved the evidence and printed the
# cause; all this has to do is say which of them happened and exit non-zero.
set +e
VANILLA_EXTRA_PROPS="gamemode=creative
generate-structures=false
spawn-monsters=false
difficulty=$GAMEPLAY_DIFFICULTY
spawn-radius=0" \
    "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$VAN_PORT" "minecraft:flat" "$GAMEPLAY_SEED"
BOOT_RC=$?
set -e
case "$BOOT_RC" in
    0) hold_phase vanilla-up ;;
    3) refuse "vanilla could not bind :$VAN_PORT — another server already has it.
         This is a COLLISION, not a defect in this tree: nothing here was measured.
         The evidence is preserved under $ROOT/oracle-work/server-failures/.
         Re-run; the ports are chosen per-tree and probed, so this should be rare." ;;
    2) refuse "vanilla did not reach 'Done (' within 120s on :$VAN_PORT.
         Evidence preserved under $ROOT/oracle-work/server-failures/." ;;
    *) refuse "vanilla died before it finished booting on :$VAN_PORT (start exit $BOOT_RC).
         The cause is printed above and the crash report, the log and the server's
         stderr are preserved under $ROOT/oracle-work/server-failures/ —
         they are NOT in $VAN_DIR, which this script wipes on the next run." ;;
esac
    : > "$WORK/harness-gamerules.txt"
    corpus_lock_assert_anchored
    for gr in "${SUBSTRATE_GAMERULES[@]}"; do
        gamerule_or_die "$VAN_DIR" "${gr%%=*}" "${gr#*=}"
    done
    # Same or-die shape as a game rule, and for the same reason: a console
    # command whose failure is not read is not a command. `time set` answers
    # `Set the time to N` on success and `Incorrect argument` on a bad one, so
    # the existing refusal grep covers it.
    time_set_or_die "$VAN_DIR" "$SUBSTRATE_DAY_TIME"
    substrate_restore_check
"$BIN" record "127.0.0.1:$VAN_PORT" "$CHECKS" --out "$WORK/vanilla.json" \
    --console "$VAN_DIR/console.fifo"
# The reference must have been alive for the whole recording. `stop` is
# deliberately forgiving of a corpse — its postcondition is "nothing running" —
# so the *assertion* lives here, where a dead vanilla means the recording is not
# a reading of vanilla. This is the redness that `stop` no longer carries.
"$ROOT/ci/vanilla-server.sh" alive "$VAN_DIR" || refuse \
    "vanilla was NOT RUNNING when its recording finished — it died mid-corpus.
     $WORK/vanilla.json describes a server that stopped answering, not the reference.
     Evidence preserved under $ROOT/oracle-work/server-failures/."
assert_recording "$WORK/vanilla.json" vanilla
hold_phase reference-done recorded
"$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"
hold_phase vanilla-stopped
}

# The cached path's half of the same thing: no server, so the rules are
# declared rather than delivered, and the restore check still runs.
declare_cached_substrate() {
    : > "$WORK/harness-gamerules.txt"
    for gr in "${SUBSTRATE_GAMERULES[@]}"; do
        printf '%s=%s\n' "${gr%%=*}" "${gr#*=}" >> "$WORK/harness-gamerules.txt"
    done
    echo "substrate: ${#SUBSTRATE_GAMERULES[@]} game rules declared, delivered by the run that recorded the cached reference"
    substrate_restore_check
}

# `record_subject` — boot this server on `$OUR_PORT`, record `$CHECKS` into
# `$WORK/rustmc.json`, and stop it. Same shape and same reason as
# `record_reference`: unchanged body, five variables, callable per shard.
record_subject() {
# --- rustmc, the subject ---------------------------------------------------
echo "== booting rustmc (superflat) on :$OUR_PORT"
# `--console`: the subject's own fifo, the other half of the asymmetry
# `crate::randomtick`'s module doc used to describe as permanent — "the
# harness quiets vanilla and does not quiet this server". `$OUR_DIR` mirrors
# `$VAN_DIR`/`ci/vanilla-server.sh`'s own layout (`console.fifo`,
# `server.pid`) on purpose: it is what lets `ci/vanilla-server.sh console`
# be reused unmodified below rather than reimplemented for a second server.
OUR_DIR="$WORK/rustmc"
mkdir -p "$OUR_DIR"
mkfifo "$OUR_DIR/console.fifo" 2>/dev/null || true
# `--view-distance 4` matches `ci/vanilla-server.sh`'s `view-distance=4`, and
# it is substrate in the same sense `difficulty=peaceful` is: the two servers
# have to be configured alike or every distance-dependent comparison measures
# the configuration. It went unset until 2026-08-04, when
# `530-walking-away-drops-a-standing-player-from-view` measured the
# difference — this server was using the generated spawn tile's radius, 2, as
# a view distance, so it tracked entities to 32 blocks where vanilla tracked
# to 64. `--radius` is a separate quantity and stays: it is how big a tile is.
# `--level-seed`: the seed vanilla is booted with (`level-seed=` above),
# which every loot table naming a `random_sequence` draws from.
./target/release/rustmc-server run --port "$OUR_PORT" --level-type flat --radius 2 \
    --level-seed "$GAMEPLAY_SEED" \
    --view-distance 4 --console "$OUR_DIR/console.fifo" \
    --seed "$GAMEPLAY_SEED" \
    --day-time "$SUBSTRATE_DAY_TIME" --advance-time false \
    --view-distance 4 \
    ${DIFFICULTY_ARGS[@]+"${DIFFICULTY_ARGS[@]}"} \
    >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"
# Readiness, and it is a proof rather than a string now. Two arms, and the
# reason there are exactly two here and three in the subset scripts is in
# `ci/server-ready.sh`:
#
#   * the server's own line, which `status::listen` prints past the bind
#     carrying `local_addr()`, matched **against the port we asked for**. A
#     line naming a different port is a different boot;
#   * the kernel's answer to whose socket that is. Our bind failing and
#     another tree's server holding :$OUR_PORT are indistinguishable from the
#     log alone, and that pair is the whole defect: `record` connects, the
#     neighbour answers, and the corpus is a measurement of somebody else.
#
# No handshake arm here. This is the gate every branch runs, under the
# box-wide corpus lock, and a `grep` and an `ss` read cannot perturb the
# subject at all — where opening a connection to it moments before the corpus
# does is a behaviour change I would have to run the full corpus to price,
# which is forty minutes of a contended lock. The two subset scripts take that
# third arm and are cheap enough to measure end to end.
#
# The timeout is now a failure. It used to fall out of the loop and record
# anyway: sixty seconds of a server that never came up produced a run whose
# only symptom was whatever `record` said about a refused connection.
RUSTMC_READY=0
for _ in $(seq 1 120); do
    grep -q "listening on 0.0.0.0:$OUR_PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null && ready_socket_owner "$OUR_PORT" "$RUSTMC_PID" && { RUSTMC_READY=1; break; }
    kill -0 "$RUSTMC_PID" 2>/dev/null || { echo "rustmc died — tail:"; tail -20 "$WORK/rustmc.log"; exit 1; }
    sleep 1
done
[ "$RUSTMC_READY" = 1 ] || refuse \
    "rustmc-server never proved it was listening on :$OUR_PORT within 120s.
     It is alive — this is not the died-at-boot path — but either it never
     printed a post-bind line for that port or the LISTEN socket there is not
     its. tail of $WORK/rustmc.log:
$(tail -20 "$WORK/rustmc.log")"
sleep 1
hold_phase rustmc-up
corpus_lock_assert_anchored
# The same substrate rules vanilla got, delivered to the subject's own
# console — the fix for the asymmetry `crate::randomtick`'s module doc used
# to describe: vanilla was quieted and this server was not, and the day this
# server's `random_tick_speed` default (vanilla's own 3,
# `crate::gamerules::DEFAULTS`) went live that asymmetry stopped being
# cosmetic — an un-quieted rustmc would random-tick every rig's grass at
# speed 3 for all ninety-two scenarios that are not `780`, reproducing the
# exact flap `SUBSTRATE_GAMERULES` was written to stop, on the other server.
# `rustmc-server` accepts the same rule names and never refuses a known one
# (`crate::gamerules::GameRules::set`'s only error paths are an unknown rule
# or the wrong shape for it, both printed to `$WORK/rustmc.log` since it
# captures the server's own stderr) — so the refusal grep here is
# `crate::console`'s own two messages, not vanilla's Brigadier text.
rustmc_gamerule_or_die() {
    local rule="$1" value="$2"
    "$ROOT/ci/vanilla-server.sh" console "$OUR_DIR" "gamerule $rule $value" || {
        echo "FAIL: could not deliver 'gamerule $rule $value' to rustmc in $OUR_DIR" >&2
        exit 1
    }
    sleep 1
    if grep -qE "console: no such game rule|console: .* takes (true or false|an integer)" "$WORK/rustmc.log" 2>/dev/null; then
        echo "FAIL: rustmc refused 'gamerule $rule $value' — see $WORK/rustmc.log" >&2
        grep -E "console: no such game rule|console: .* takes" "$WORK/rustmc.log" | tail -4 >&2
        exit 1
    fi
}
for gr in "${SUBSTRATE_GAMERULES[@]}"; do
    rustmc_gamerule_or_die "${gr%%=*}" "${gr#*=}"
done
"$BIN" record "127.0.0.1:$OUR_PORT" "$CHECKS" --out "$WORK/rustmc.json" \
    --console "$OUR_DIR/console.fifo"
assert_recording "$WORK/rustmc.json" rustmc
hold_phase subject-done
kill "$RUSTMC_PID" 2>/dev/null || true
rm -f "$OUR_DIR/server.pid"
RUSTMC_PID=""
}

# --- the shards ---------------------------------------------------------------
#
# **`RUSTMC_CORPUS_SHARDS=N` splits one corpus run across N isolated vanilla +
# rustmc pairs and records them all at once.** Added 2026-09-24 on branch
# `corpus-parallel`, because a recording is wall clock and not CPU: ~12 s a
# scenario per side, almost all of it `settle_ms`, `QUIET` and a 32-block walk
# at ten blocks a second, at a load the box does not notice. Nothing in one
# recording waits on anything in another, so N of them in parallel cost the
# wall clock of the longest shard.
#
# What "isolated" has to cover, and each item is a thing a second recording in
# the same tree would otherwise share:
#
#   * **ports** — `pick_port_pair $SHARDS` returns N probed-free pairs;
#   * **the vanilla world and its console fifo** — `$WORK/shard-i/vanilla`;
#   * **the subject's console fifo, pid file and log** — `$WORK/shard-i/rustmc`
#     and `$WORK/shard-i/rustmc.log`;
#   * **the recordings, their `.prov` stamps and `harness-gamerules.txt`** —
#     all under `$WORK/shard-i/`, merged in corpus order into `$WORK/vanilla.json`
#     and `$WORK/rustmc.json` only after every shard has passed its own
#     `assert_recording`.
#
# A shard is a CONTIGUOUS run of the sorted corpus, so each shard's players walk
# the same westward march the serial run does, only starting again at the first
# workspace. Workspaces are 32 blocks apart and 32 is a multiple of 16, so every
# scenario still runs at the same section-relative coordinates it always had —
# the property `record::run_one` documents `section_blocks_update`'s byte order
# depending on. The first scenario of each shard is on a fresh connection,
# which the recorder writes down (`fresh_session`) and `compare` rebases.
#
# **Whether this changes any verdict is not argued here, it is measured:**
# `ci/corpus-shard-equivalence.sh` records one scenario set serially and
# sharded and refuses on a scenario whose verdict or whose deterministic
# projection moved. Its evidence is in `ci/checks/corpus-shard-equivalence.toml`.
#
# `N=1` is the serial path, unchanged: the same two functions called in this
# shell, on `$WORK` itself.
# (`SHARDS` itself is parsed above the port pick, which needs the count.)

# The sharded path runs each recording in a subshell with the five variables
# `record_reference`/`record_subject` read pointed at one shard, and its own
# EXIT trap so a shard that dies stops its own server. The parent waits on every
# one of them and never reads a shard's recording until that shard exited 0.
run_shards() {   # run_shards <kind:reference|subject> ; appends to SHARD_PIDS
    local kind="$1" i
    for i in $(seq 0 $((SHARDS - 1))); do
        (
            WORK="$SHARD_TOP/shard-$i"
            CHECKS="$WORK/checks"
            VAN_DIR="$WORK/vanilla"
            read -r VAN_PORT OUR_PORT <<<"${SHARD_PAIRS[$i]}"
            N_CHECKS=$(find "$CHECKS" -maxdepth 1 -name '*.toml' | wc -l)
            RUSTMC_PID=""
            # The phase log describes the run, not its shards: the parent writes
            # one `reference-done` and one `subject-done` when the last shard of
            # each kind is in.
            hold_phase() { :; }
            if [ "$kind" = reference ]; then
                trap '"$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
                      [ -d "$VAN_DIR/crash-reports" ] && "$ROOT/ci/vanilla-server.sh" preserve "$VAN_DIR" "shard '"$i"' crash report" || true' EXIT
                record_reference
            else
                trap '[ -n "$RUSTMC_PID" ] && kill "$RUSTMC_PID" 2>/dev/null || true' EXIT
                record_subject
            fi
        ) > "$SHARD_TOP/shard-$i/$kind.log" 2>&1 &
        SHARD_PIDS+=("$!")
        echo "  shard $i/$SHARDS $kind: pid $! on :$(set -- ${SHARD_PAIRS[$i]}; [ "$kind" = reference ] && echo "$1" || echo "$2"), log $SHARD_TOP/shard-$i/$kind.log"
    done
}
# Wait for every shard of one kind. A shard's refusal is re-raised here as this
# run's refusal, with its own words, because a `T2-GAMEPLAY REFUSED` line that
# is only in a shard log is a verdict nobody downstream reads.
join_shards() {  # join_shards <kind> <first-pid-index>
    local kind="$1" base="$2" i rc bad=0 first_rc=0
    for i in $(seq 0 $((SHARDS - 1))); do
        rc=0; wait "${SHARD_PIDS[$((base + i))]}" || rc=$?
        if [ "$rc" -ne 0 ]; then
            echo "  !! shard $i $kind exited $rc — tail of $SHARD_TOP/shard-$i/$kind.log:" >&2
            tail -25 "$SHARD_TOP/shard-$i/$kind.log" | sed 's/^/     /' >&2
            bad=$((bad + 1)); [ "$first_rc" -ne 0 ] || first_rc="$rc"
        else
            grep -E '^RECORDED ' "$SHARD_TOP/shard-$i/$kind.log" | sed "s/^/  shard $i $kind: /" || true
        fi
    done
    [ "$bad" -eq 0 ] && return 0
    if [ "$first_rc" -eq 4 ]; then
        refuse "$bad of $SHARDS $kind shard(s) refused — the shard logs above say why."
    fi
    echo "FAIL: $bad of $SHARDS $kind shard(s) failed (first exit $first_rc)" >&2
    exit "$first_rc"
}
# Concatenate the shards' recordings in corpus order, refusing anything that is
# not exactly the corpus: a shard that recorded a scenario twice, skipped one,
# or recorded them out of order is a partition bug, and a merged file that hid
# it would be read as a recording of this corpus.
merge_shards() {  # merge_shards <vanilla|rustmc>
    python3 - "$CHECKS" "$SHARD_TOP" "$SHARDS" "$1" <<'PY' || refuse "the $1 shards do not merge into this corpus"
import json, os, sys, tomllib
checks, top, n, who = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
# The recorder names a scenario by its `name` field, falling back to the file
# stem (`Scenario::load`), in sorted-path order (`Scenario::load_dir`).
want = []
for f in sorted(f for f in os.listdir(checks) if f.endswith(".toml")):
    with open(os.path.join(checks, f), "rb") as fh:
        want.append(tomllib.load(fh).get("name") or f[:-5])
merged, names = None, []
for i in range(n):
    with open(os.path.join(top, f"shard-{i}", f"{who}.json")) as fh:
        t = json.load(fh)
    if merged is None:
        merged = dict(t, scenarios=[])
    elif (t["version"], t["protocol"], t["recorder"]) != (merged["version"], merged["protocol"], merged["recorder"]):
        print(f"  !! shard {i} is a different server or recorder: "
              f"{t['version']}/{t['protocol']}/{t['recorder']}", file=sys.stderr)
        raise SystemExit(1)
    merged["scenarios"].extend(t["scenarios"])
names = [s["name"] for s in merged["scenarios"]]
if names != want:
    missing = sorted(set(want) - set(names)); extra = sorted(set(names) - set(want))
    print(f"  !! merged {who} has {len(names)} scenarios, corpus has {len(want)}; "
          f"missing {missing[:5]} extra {extra[:5]} order-ok={sorted(names) == names}", file=sys.stderr)
    raise SystemExit(1)
merged["target"] = f"{merged['target']} (+{n - 1} shards)" if n > 1 else merged["target"]
with open(os.path.join(top, f"{who}.json"), "w") as fh:
    json.dump(merged, fh, indent=2)
print(f"  merged {len(names)} {who} scenarios from {n} shards")
PY
}

if [ "$SHARDS" -eq 1 ]; then
if [ -n "$REF_CACHED" ]; then
    declare_cached_substrate
    # The served recording is stamped for THIS tree, because it now lives in
    # this tree's work directory and `recording_still_ours` below is asking
    # whether anything overwrote it between being put there and being compared
    # — a question about this run, which a cached origin does not change.
    # `assert_recording` is not skipped: it is what refuses a recording whose
    # scenario count is not this corpus's, and a cache serving the right key
    # for the wrong denominator is exactly the failure worth keeping a second
    # instrument for.
    assert_recording "$WORK/vanilla.json" "vanilla (cached)"
    hold_phase reference-done served
else
    record_reference
# Stored only after `alive` and `assert_recording` have both passed. A recording
# from a vanilla that died mid-corpus, or one that does not cover this corpus,
# must never become the artefact every later gate is handed — a cache entry is
# read far more often than it is written, so the bar to enter it is the bar the
# gate itself applies, not a lower one.
refcache_store "$ROOT" "$BIN" "$GAMEPLAY_SEED" "$WORK/vanilla.json"
fi
record_subject
else
    SHARD_TOP="$WORK"
    # The partition: contiguous, sizes differing by at most one, in the sorted
    # order `Scenario::load_dir` reads. Links, not copies, so the recorder
    # reads the corpus's own bytes and names each scenario by the same stem.
    mapfile -t _ALL < <(find "$CHECKS" -maxdepth 1 -name '*.toml' -printf '%f\n' | LC_ALL=C sort)
    _k=0
    for i in $(seq 0 $((SHARDS - 1))); do
        rm -rf "$SHARD_TOP/shard-$i"; mkdir -p "$SHARD_TOP/shard-$i/checks"
        _n=$(( (${#_ALL[@]} + SHARDS - 1 - i) / SHARDS ))
        for f in "${_ALL[@]:$_k:$_n}"; do ln -s "$CHECKS/$f" "$SHARD_TOP/shard-$i/checks/$f"; done
        _k=$((_k + _n))
    done
    [ "$_k" -eq "${#_ALL[@]}" ] || refuse "the shard partition covered $_k of ${#_ALL[@]} scenarios"
    echo "== $SHARDS shards of $N_CHECKS scenarios, recorded concurrently"
    hold_phase shards-begin "$SHARDS"
    SHARD_PIDS=()
    if [ -n "$REF_CACHED" ]; then
        declare_cached_substrate
        assert_recording "$WORK/vanilla.json" "vanilla (cached)"
        hold_phase reference-done served
        run_shards subject
        join_shards subject 0
    else
        run_shards reference
        run_shards subject
        join_shards reference 0
        merge_shards vanilla
        assert_recording "$WORK/vanilla.json" "vanilla ($SHARDS shards)"
        hold_phase reference-done recorded
        join_shards subject "$SHARDS"
    fi
    merge_shards rustmc
    assert_recording "$WORK/rustmc.json" "rustmc ($SHARDS shards)"
    hold_phase subject-done
    # Stored under the same bar as the serial path — only after every shard's
    # own `alive` and `assert_recording` and the merged file's.
    [ -n "$REF_CACHED" ] || refcache_store "$ROOT" "$BIN" "$GAMEPLAY_SEED" "$WORK/vanilla.json"
fi


# --- the diff --------------------------------------------------------------
# Last gate before the number: both recordings must still be the ones this run
# made. Two trees sharing one `oracle-work` overwrite each other here, and the
# result reads as a regression rather than as a collision.
recording_still_ours "$WORK/vanilla.json" vanilla
recording_still_ours "$WORK/rustmc.json" rustmc
[ -s "$BASELINE" ] || refuse "no baseline at $BASELINE — every check would look new"

ARGS=(compare "$CHECKS" --reference "$WORK/vanilla.json" --subject "$WORK/rustmc.json"
      --json "$WORK/report.json")
ARGS+=(--baseline "$BASELINE")
"$BIN" "${ARGS[@]}"
hold_phase compare-done

# --- the command surface, before anything is recorded ----------------------
#
# Cheap, and it fails for a reason that would otherwise be misread further
# down: if the generated surface has drifted from the jar, the command-tree
# ratchet below is comparing against a baseline whose meaning has changed.
#
# The dump is regenerated rather than assumed. `data/vanilla` is not tracked,
# so a fresh worktree has no executed derivation, and `--check` refuses
# without one instead of quietly comparing a weaker document.
[ -s "$ROOT/data/vanilla/command_surface.json" ] || "$ROOT/ci/command-surface-dump.sh"
python3 "$ROOT/ci/command-surface.py" --check \
    || refuse "the generated command surface has drifted from the jar"

# And the same check's refusals, injected. `--check` above prints one line and
# exits 0, and every refusal in it guards an input nobody has produced yet: the
# two derivations have agreed at 90 roots every time. Two of them did fire
# during development and both fixes removed the only input that violated them,
# which is the position ci/check-figures.py documents for its own rules 9-13.
# ~1 minute, and it names the three bytecode refusals it cannot reach.
python3 "$ROOT/ci/command-surface-falsify.py" > "$WORK/cmdsurface-falsify.log" 2>&1 \
    || { cat "$WORK/cmdsurface-falsify.log" >&2
         refuse "ci/command-surface-falsify.py: an injected fault went green"; }
tail -1 "$WORK/cmdsurface-falsify.log"

# --- the command tree, which no scenario can observe ------------------------
#
# `ClientboundCommandsPacket` arrives during the join, before the first
# scenario's measured window opens, so it can never appear in a scenario's
# `obs` and no check above has ever looked at it. `Session` captures it in
# `liveness` and reports it per scenario, and this compares the two servers'
# once rather than turning one join-time fact into 108 identical verdicts.
#
# Run before the floor because it is about the *denominator's* honesty: until
# 2026-08-18 the whole command registry was gated only by
# `ci/commands-golden.sh`, which encodes our own registration with vanilla's
# encoder and so cannot name a command vanilla has and we do not.
#
# Its exit code is respected. It refuses on a recording that never saw the
# packet, on vanilla's side moving, and on us declaring fewer roots than the
# ratchet — never on the gap itself, which is printed and held.
"$ROOT/ci/command-tree-parity.py" "$WORK/vanilla.json" "$WORK/rustmc.json"

# --- the floor, beside the score and never subtracted from it ---------------
#
# **A corpus score without its floor is not a number**, on exactly the terms
# `docs/PARITY.md` §4 imposes on every worldgen figure: a residual smaller than
# what two vanilla runs disagree about is not a defect. `62/88` reads as
# 26 scenarios this server gets wrong, and some of them are scenarios nothing
# gets right — not this server, not vanilla.
#
# Printed here rather than left to `ci/corpus-floor.sh` because the number that
# needs the annotation is the one printed two lines above, and a caveat that
# lives in another file's output is a caveat nobody reads beside the figure.
# `ci/corpus-floor-report.py` writes to this script's stdout, so moving the
# code out of a heredoc did not move the output; read its header for what the
# annotation is and for why the floor's *projection* is checked and not only
# its denominator.
#
# **It is an annotation on the denominator, not a deduction from the
# numerator.** Nothing in `ci/checks/gameplay-floor.toml` or
# `ci/checks/gameplay-unstable.toml` can change a verdict, move the ratchet in
# `gameplay-baseline.toml`, or make a scenario stop failing —
# `ci/corpus-floor-falsify.sh` case 11 asserts that neither this script nor the
# reporter so much as reads the unstable list. The floor makes the score
# honest; it does not make it better.
#
# The identity below is re-derived from **the binary this gate is about to
# run**, not from the tree it is standing in, which is the half that survives
# `rsync -a` leaving sources older than the artefacts built from them.
# `compare::PROJECTION_SOURCES` says what goes into it and what deliberately
# does not.
FLOOR_FILE="$ROOT/ci/checks/gameplay-floor.toml"
# Not `|| true`: a binary that cannot state its own projection is a binary too
# old to be trusted about anything else here, and the empty string must not
# compare equal to a missing stamp.
RUNNING_PROJECTION="$("$BIN" projection-id 2>/dev/null || echo unavailable)"
# The reference recording goes in too, and it is the fourth positional argument
# rather than an optional flag because an input a caller may omit is an input a
# caller will omit. The projection stamp answers "same comparator?"; only the
# recording can answer "over the same fields?" — `add_entity.y_rot` exists in
# this run's vocabulary and does not exist in the standing floor's ensemble, so
# the comparator's yaw exemption is live here and was inert there.
# docs/PARITY-ROADMAP.md §15.4.
"$ROOT/ci/corpus-floor-report.py" "$FLOOR_FILE" "$WORK/report.json" \
    "$RUNNING_PROJECTION" "$WORK/vanilla.json" || true

# **What the projection could not see, printed beside what it did.**
#
# `docs/PARITY-ROADMAP.md` §51. Both read the pair this run just produced and
# neither can affect the verdict — they are `|| true` and print to stdout, in
# the same spirit as the floor report above.
#
# They are here rather than left as tools somebody remembers because the whole
# finding of §51 is that these questions get asked years late and by accident.
# `observe-coverage` says which declared kinds never appeared and which checks
# rest on a single observation — 550 and 551 declare four kinds, three of which
# vanilla never emits, and rest on `sound` alone. `hidden-stream-audit` reads
# the client `observe_from` discards, which for 102 of the 108 checks is a
# fully recorded stream no comparison looks at. Its first hit was 551's step
# sounds — read from one pair as a plain divergence and corrected the same day
# to a distribution: vanilla 4 in 32 runs and 5 in 39, ours 5 in 66 of 66. That
# correction is why it prints rather than gates, and why its own header calls
# it a screen.
#
# Skipped on a subset run: both are censuses over the whole corpus and a
# partial one would print a smaller number that reads like a smaller problem.
if [ -z "${RUSTMC_CHECKS:-}" ]; then
    echo "== what the projection did not look at (PARITY-ROADMAP 51, advisory)"
    python3 "$ROOT/ci/observe-coverage.py" "$WORK" 2>&1 | sed 's/^/   /' || true
    python3 "$ROOT/ci/hidden-stream-audit.py" "$WORK" 2>&1 | sed 's/^/   /' || true
fi

if [ -n "${RUSTMC_CHECKS:-}" ]; then
    # Deliberately NOT the string a gate log is grepped for. `ci/handoff.sh`
    # and every waiter in `docs/AGENT-BRIEF.md` match on
    # `T2-GAMEPLAY (GREEN|REFUSED)`, so a subset run cannot satisfy one by
    # accident, and a human reading the log is told which run this was and
    # what it did not do.
    echo "T2-GAMEPLAY SUBSET OK — $N_CHECKS scenario(s) from $RUSTMC_CHECKS"
    echo "  NOT A GATE. The corpus ratchet in ci/checks/gameplay-baseline.toml"
    echo "  cannot see a scenario this run did not execute. Re-run with no"
    echo "  RUSTMC_CHECKS before quoting a corpus verdict."
    exit 0
fi

echo "T2-GAMEPLAY GREEN"
