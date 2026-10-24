#!/usr/bin/env bash
# reference-cache.sh — record the vanilla reference once per (jar, corpus,
# recorder, substrate), not once per gate.
#
# # Why this exists
#
# A merge gate holds `ci/corpus-lock.sh` — the box-wide one — for the whole of
# `ci/t2-gameplay.sh`, and that hold is the project's throughput ceiling: the
# `gate-lanes` branch measured an Amdahl limit of 1.40x at any number of lanes,
# because the lock is the machine. The only thing that moves the number is the
# hold itself.
#
# Measured 2026-08-16 (method and figures in `docs/CORPUS-LOCK-HOLD.md`), the
# hold splits almost exactly in half:
#
#     vanilla reference recording   12.09 s/scenario   21.3 min at N=106
#     rustmc subject recording      12.21 s/scenario   21.6 min at N=106
#     compare + floor report                           <0.1 min
#
# n=5 gate trees by phase mtime, corroborated by n=26 cold selfcheck boots at
# three corpus sizes (12.01 / 12.14 / 12.07 s per scenario at N=88 / 93 / 109 —
# linear in N with a zero intercept, ±0.1 min over six runs of one size).
#
# **The reference half does not depend on the candidate branch.** It is what an
# unmodified vanilla 26.2 does when the recorder drives the scenario corpus at
# it. Nothing our server produces reaches vanilla: `record` opens two clients
# against `127.0.0.1:$VAN_PORT`, drives the steps, and writes what came back.
# The subject half is a different server and is branch-dependent by
# construction. So half the most contended resource in the project is spent
# re-deriving an artefact that was already derived, under an exclusive lock, for
# every gate of every branch.
#
# # Why a cache here is the dangerous kind, and what makes this one safe
#
# **A stale reference is the worst failure this project can have.** Every parity
# number rests on the reference being what the unmodified jar actually produced
# for the scenarios *as they are now*. A cache that served a reference recorded
# against a different jar, a different scenario file or a different recorder
# would not make a check fail — it would make every check green in a way that
# could no longer go red, which is this repo's central defect class
# (`docs/AGENT-BRIEF.md`, `ci/corpus-falsifiable.py`).
#
# This project has already paid for the unsafe version of exactly this idea. On
# 2026-08-03 `oracle-work` and `data/vanilla` were *shared between trees by
# path*, and a gate reported 12/55 with 11 regressed for a branch that was fine,
# because a path with no content identity hands you whatever was written into it
# last. The difference here is not care, it is structure:
#
#   * **shared by path** -> you get the last write, and nothing can tell;
#   * **shared by content hash** -> you get exactly the artefact whose inputs
#     you named, or you get nothing.
#
# So every input is hashed by CONTENT. No mtimes (`rsync -a` moves those
# backwards and cargo has already been fooled by it once), no version strings,
# no branch names, no "the tables looked fine".
#
# And the key is not opaque. Every component is stored beside the entry and
# **re-derived and compared component by component on every serve**, so a miss
# names the component that moved rather than printing two hex strings. A cache
# you cannot debug is a cache somebody will disable.
#
# # The inputs, and why each one is in the key
#
#   jar         the pinned server-26.2.jar. The reference IS this file's
#               behaviour. sha256 of the whole jar, not a version string: a
#               version string is an assertion, a hash is a measurement.
#   scenarios   every file under ci/checks/gameplay, path and content. A
#               renamed scenario is a different corpus; so is an edited step.
#   recorder    tools/oracle-runner/src/*.rs. The recording is what the
#               recorder chose to write down, so the recorder is an input to
#               it. This is the same digest `ci/corpus-floor.sh` already forms
#               as `recorder_digest`.
#   projection  `oracle-runner projection-id`, from THE BINARY ABOUT TO RUN.
#               `compare::PROJECTION_SOURCES` is compare.rs + scenario.rs +
#               trace.rs, and `scenario.rs` drives recording as well as
#               comparison, so the projection and the recording genuinely
#               share an input. It is also in the key for a reason that does
#               not depend on that argument being right: the standing
#               projection is `proj:f7fbb29926074663`, every floor in the repo
#               predates it, and an over-specified key costs hit rate while an
#               under-specified one costs correctness. Those are not
#               comparable prices. Taken from the binary and not the tree
#               because `rsync -a` can leave sources newer than the artefact
#               built from them.
#   harness     ci/t2-gameplay.sh, ci/vanilla-server.sh, this file, and since
#               2026-09-05 ci/gameplay-substrate.sh and ci/gameplay-difficulty.sh,
#               which hold the game rules and the difficulty vanilla boots
#               under — both moved out of ci/t2-gameplay.sh, so both had to be
#               added here in the same commit or the key would have stopped
#               covering the substrate. These
#               ARE the substrate: superflat, creative, peaceful,
#               spawn-radius=0, view-distance=4, the four game rules, the
#               world seed. A reference is only a reference for the substrate
#               it was taken on.
#   tables      data/vanilla/{items,block_props,entity_types}.json — exactly
#               the three `client::Tables::load` opens, and therefore exactly
#               the ones that decide which item id and which block state the
#               harness sends TO vanilla. This is the branch-dependence that
#               the "the reference cannot depend on our tree" argument misses,
#               and it is real: measured across six trees on 2026-08-16 the
#               three hash identical in five and differ in `rustmc-items`,
#               the tree whose branch was about the item registry. That is the
#               key working — a miss for the branch that moved the input, a
#               hit for everyone else.
#   seed        GAMEPLAY_SEED, which pins the world and so the spawn point.
#   n_checks    the corpus count the recording must cover. Redundant against
#               `scenarios` and kept anyway: it is the one component a human
#               reads at a glance, and `assert_recording` refuses on it
#               downstream regardless.
#
# **What is deliberately NOT in the key:** anything about the candidate branch —
# its name, its tip, its commit, `crates/`, `data/vanilla`'s other sixteen
# tables. If any of those belonged in the key the premise would be wrong and the
# cache should not exist.
#
# # Falsification
#
# `ci/reference-cache-falsify.sh` mutates each component in turn and asserts a
# miss naming that component, asserts a hit when nothing moves, and asserts that
# a tampered or truncated entry is refused rather than served. A cache without
# that test does not land, however fast it is. `ci/corpus-lock-falsify.sh` and
# `ci/lanes-falsify.sh` are the patterns it follows.
#
# # Status
#
# **Opt-in.** `RUSTMC_REFERENCE_CACHE=1` enables lookup and store; unset, this
# file changes nothing and `ci/t2-gameplay.sh` boots vanilla exactly as before.
# That is repo policy for new behaviour ("new behaviour lands opt-in until it is
# measured") and it is doubly right here, because the measurement that would
# retire the flag is a live merge gate run both ways and this branch is not
# allowed to run one.

# --- where -------------------------------------------------------------------
# Box-wide and outside every tree, because the entire point is that sixty trees
# share one recording. Overridable so the falsify script gets its own namespace
# and cannot see, write or evict real entries — the same isolation
# `ci/corpus-lock-falsify.sh` gets.
REFCACHE_DIR="${REFCACHE_DIR:-$HOME/.rustmc-reference-cache}"

_rc_h() { sha256sum | cut -c1-16; }

# Print `name=hash` for every key component, one per line, in a fixed order.
# This is the whole key: `refcache_key` is just a hash of it, and it is stored
# verbatim beside every entry so a miss can name what moved.
#
# $1 = tree root, $2 = path to the oracle-runner binary, $3 = seed
refcache_components() {
    local root="$1" bin="$2" seed="$3"
    local checks="$root/ci/checks/gameplay"
    local jar="$root/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"

    # A missing input must NOT hash to the empty string and quietly agree with
    # another tree that is also missing it. Each of these prints a value that
    # cannot collide with a real hash, and `refcache_lookup` refuses on any of
    # them rather than treating it as a component that happens to match.
    if [ -r "$jar" ]; then printf 'jar=%s\n' "$(sha256sum "$jar" | cut -c1-16)"
    else printf 'jar=MISSING\n'; fi

    if [ -d "$checks" ]; then
        # RELATIVE path AND content: `find | sort | xargs sha256sum` emits
        # both, so a rename WITHIN checks/gameplay moves the key. `-type f`
        # and not a glob, so a scenario in a subdirectory is not silently
        # outside the identity.
        #
        # `sed "s#$checks/##"` strips `$checks` itself off every emitted
        # filename before hashing. Without it `sha256sum`'s own output line
        # carries the ABSOLUTE path, so the hash silently also covers `$root`
        # — the one thing this design says explicitly must not be in the key.
        # Found by `ci/reference-cache-falsify.sh`'s `root-invariance` case:
        # two structurally identical trees at different absolute paths (which
        # is every pair of trees in this project's own fleet, since
        # `RUSTMC_REMOTE_DIR` is meant to be unique per agent and the two
        # lanes are `rustmc-lane-1`/`rustmc-lane-2`) computed DIFFERENT
        # `scenarios` hashes and could never share an entry — quietly
        # defeating "sixty trees share one recording" for this component
        # specifically, in exactly the direction that costs throughput rather
        # than correctness, which is why the 22-case suite that shipped with
        # the cache never caught it: nothing in it used two roots.
        printf 'scenarios=%s\n' "$(find "$checks" -type f -print0 | sort -z \
            | xargs -0 sha256sum 2>/dev/null | sed "s#$checks/##" | _rc_h)"
        printf 'n_checks=%s\n' "$(find "$checks" -maxdepth 1 -name '*.toml' | wc -l)"
    else
        printf 'scenarios=MISSING\n'; printf 'n_checks=MISSING\n'
    fi

    if compgen -G "$root/tools/oracle-runner/src/*.rs" >/dev/null; then
        printf 'recorder=%s\n' "$(cat "$root"/tools/oracle-runner/src/*.rs | _rc_h)"
    else printf 'recorder=MISSING\n'; fi

    # THE RECORDER IS NOT ONLY ITS OWN SOURCE. Added 2026-08-24.
    #
    # `recorder=` above hashes `tools/oracle-runner/src/*.rs`, and `projection=`
    # below is derived from three of those same modules compiled into the
    # binary. Both stop at the crate boundary — and the recorder does not.
    # `trace.rs` is the DECODER and it names `rustmc_proto` eleven times;
    # `client.rs` names it four more. So the bytes a vanilla recording is made
    # of are a function of `crates/rustmc-proto` as much as of the runner's own
    # source, and until this component existed a branch could change how a
    # packet is decoded without moving any part of the key.
    #
    # WHAT THAT WOULD HAVE COST, stated in the direction it actually fails:
    # the cached reference is a recording ALREADY DECODED by the older binary,
    # so a decoder change makes the served reference and the freshly recorded
    # subject disagree about fields neither side's server changed. That is a
    # spurious RED, not a spurious GREEN — the safe direction — but it is a red
    # that names the branch while measuring the cache, which is the most
    # expensive kind of red this project has.
    #
    # It is added at zero cost to the hit rate, and that is measured rather
    # than hoped: of the 132 branches unlanded against main on 2026-08-24,
    # **none** touches `crates/rustmc-proto` — while main itself took 21
    # commits to it in the preceding 30 days. So the gap is live and the fix
    # is free today, which is the only cheap moment it will have.
    if compgen -G "$root/crates/rustmc-proto/src/*.rs" >/dev/null; then
        printf 'proto=%s\n' "$(cat "$root"/crates/rustmc-proto/src/*.rs | _rc_h)"
    else printf 'proto=MISSING\n'; fi

    # From the binary, not the tree. Not `|| true`: a binary that cannot state
    # its own projection is too old to be trusted about anything else here, and
    # the empty string must not compare equal to a missing stamp — the same rule
    # `ci/t2-gameplay.sh` applies to `RUNNING_PROJECTION`.
    local proj=""
    [ -x "$bin" ] && proj="$("$bin" projection-id 2>/dev/null || true)"
    printf 'projection=%s\n' "${proj:-MISSING}"

    # `ci/gameplay-substrate.sh` joined the list on 2026-09-05 and is not
    # optional: the four game rules the harness sends to vanilla moved out of
    # `ci/t2-gameplay.sh` into it, and `ci/t2-gameplay.sh`'s own comment beside
    # them said "change a rule, change the substrate, miss the cache, boot
    # vanilla". Moving them out without moving this line would have made that
    # sentence false — a rule edited in the new file, a key unmoved, and the
    # cache serving a reference recorded under the old substrate. Same for
    # `ci/gameplay-difficulty.sh`, which chooses the difficulty vanilla boots
    # at and is therefore part of what the reference IS.
    # AND THE VALUE, NOT ONLY THE FILE. `GAMEPLAY_DIFFICULTY` is an environment
    # variable, so hashing `ci/gameplay-difficulty.sh` covers a change to the
    # DEFAULT and nothing else: a run with `GAMEPLAY_DIFFICULTY=easy` would
    # otherwise be served a reference recorded at `peaceful`, and at `peaceful`
    # vanilla creates no monster at all (`EntityType.canSpawn`, offsets 13-31).
    # That is a cached reference of a different world, which is the one thing
    # this key exists to make impossible.
    printf 'difficulty=%s\n' "${GAMEPLAY_DIFFICULTY:-peaceful}"

    local hs=""
    for f in ci/t2-gameplay.sh ci/vanilla-server.sh ci/reference-cache.sh \
             ci/gameplay-substrate.sh ci/gameplay-difficulty.sh; do
        [ -r "$root/$f" ] || { hs="MISSING"; break; }
    done
    if [ "$hs" = MISSING ]; then printf 'harness=MISSING\n'
    else printf 'harness=%s\n' "$(cat "$root/ci/t2-gameplay.sh" \
            "$root/ci/vanilla-server.sh" "$root/ci/reference-cache.sh" \
            "$root/ci/gameplay-substrate.sh" "$root/ci/gameplay-difficulty.sh" | _rc_h)"; fi

    # Exactly the four `client::Tables::load` opens. Listed here and asserted
    # against the loader by `ci/reference-cache-falsify.sh` case `tables-set`,
    # so a fifth table appearing in `Tables::load` cannot silently fall outside
    # the key — which would be a table that changes what the harness sends to
    # vanilla while the cache goes on serving the old reference.
    #
    # **`protocol_ids.json` is the fourth and the guard is why it is here.**
    # `corpus-sound-pitch` added it to `Tables::load` — `pitch.rs` resolves a
    # sound event id to a call site and needs the registry to do it — and did
    # not add it here. `tables-set` failed on the next T0 with the two sets
    # printed side by side. Left alone, a change to the sound registry would
    # have moved what the recorder can classify while this cache went on
    # serving a reference recorded before it, which is the precise failure this
    # key exists to make impossible.
    local tok=1
    for t in items block_props entity_types protocol_ids; do
        [ -r "$root/data/vanilla/$t.json" ] || tok=0
    done
    if [ "$tok" = 1 ]; then
        printf 'tables=%s\n' "$(cat "$root/data/vanilla/items.json" \
            "$root/data/vanilla/block_props.json" \
            "$root/data/vanilla/entity_types.json" \
            "$root/data/vanilla/protocol_ids.json" | _rc_h)"
    else printf 'tables=MISSING\n'; fi

    printf 'seed=%s\n' "$seed"

    # How the recording was taken, since 2026-09-24: `RUSTMC_CORPUS_SHARDS`
    # splits the corpus into that many contiguous runs, each starting on a
    # fresh connection at the first workspace. The first scenario of every
    # shard is then recorded with `fresh_session = true` where a serial run
    # recorded `false`, so the two recordings are not the same artefact even
    # where every verdict agrees (`ci/corpus-shard-equivalence.sh`). Serving
    # one layout's reference to another layout's subject would make
    # `compare::session_path_divergence` fire on every shard boundary.
    printf 'shards=%s\n' "${RUSTMC_CORPUS_SHARDS:-1}"
}

# The key: a hash of the component block. Prefixed so it is never mistaken for
# a bare sha in a log.
refcache_key() { refcache_components "$@" | sha256sum | sed 's/^/ref:/;s/ .*//'; }

# Say WHICH component moved, by diffing this run's components against the
# nearest stored entry's. "Nearest" is the entry sharing the most component
# lines; with none stored there is nothing to compare and it says so.
#
# This is diagnosis, never a decision. Nothing here can cause a serve — it runs
# only on the path that has already returned a miss — so a wrong guess about
# which entry is nearest costs a confusing message and cannot cost a stale
# reference. That asymmetry is why it is allowed to guess at all.
_rc_explain_miss() {
    local comp="$1" best="" bestn=-1 d n
    for d in "$REFCACHE_DIR"/*/; do
        [ -r "$d/components" ] || continue
        n="$(comm -12 <(printf '%s\n' "$comp" | sort) <(sort "$d/components") | wc -l)"
        if [ "$n" -gt "$bestn" ]; then bestn="$n"; best="$d"; fi
    done
    if [ -z "$best" ]; then
        echo "    nothing cached yet for this corpus — this run records the reference and stores it"
        return 0
    fi
    echo "    nearest stored reference differs in:"
    # Report by NAME. The hashes are in the entry for anyone who wants them;
    # what a reader needs first is which of the inputs moved. The count is
    # deliberately not written here: it was "eight" until `proto` made it nine,
    # and a number in a comment is a number nobody re-checks.
    local k mine theirs
    while IFS= read -r k; do
        mine="$(printf '%s\n' "$comp" | sed -n "s/^$k=//p")"
        theirs="$(sed -n "s/^$k=//p" "$best/components")"
        [ "$mine" = "$theirs" ] && continue
        printf '      %-11s this run %s   cached %s\n' "$k" "$mine" "$theirs"
    done < <(printf '%s\n' "$comp" | cut -d= -f1)
    echo "    (cached entry ${best#"$REFCACHE_DIR"/})"
}

# Serve a cached reference into $4, or fail with a reason.
#
# Exit 0 = hit and $4 now holds the recording. Exit 1 = miss, with the cause on
# stderr. NEVER exits 0 without a verified file in place.
#
# $1 root  $2 bin  $3 seed  $4 dest
refcache_lookup() {
    local root="$1" bin="$2" seed="$3" dest="$4"
    [ -n "${RUSTMC_REFERENCE_CACHE:-}" ] || { echo "reference-cache: disabled (set RUSTMC_REFERENCE_CACHE=1)"; return 1; }

    local comp; comp="$(refcache_components "$root" "$bin" "$seed")"

    # A key with a MISSING component is not a key. Refusing here rather than
    # hashing it is the difference between "no cache entry" and "two trees that
    # are both missing the jar agree with each other".
    if printf '%s\n' "$comp" | grep -c '=MISSING$' >/dev/null; then
        echo "reference-cache: MISS — input unavailable:" >&2
        printf '%s\n' "$comp" | grep '=MISSING$' | sed 's/^/    /' >&2
        return 1
    fi

    local key; key="$(printf '%s\n' "$comp" | sha256sum | sed 's/^/ref:/;s/ .*//')"
    local entry="$REFCACHE_DIR/${key#ref:}"
    if [ ! -d "$entry" ]; then
        # **"No entry" is not a diagnosis.** A key is a hash of every
        # component, so a miss says only that at least one of them moved, and
        # a message that cannot say which one turns every cache miss into a
        # thing somebody has to reproduce by hand. Worse, it makes the cache
        # untestable in the way that matters: a test asserting only "it missed"
        # is satisfied by a cache that misses for some unrelated reason — tier
        # 1 in `ci/corpus-falsifiable.py`'s sense, a check scoring a claim it
        # never reads.
        #
        # So name it. Compare against the nearest stored entry — the one
        # sharing the most components — and print the components that differ.
        echo "reference-cache: MISS — no entry for $key"
        _rc_explain_miss "$comp"
        return 1
    fi

    # Re-derive and compare COMPONENT BY COMPONENT. The key already matched, so
    # this cannot normally disagree — it is here for the case where it can: an
    # entry written by an older version of this file, or a hand-edited
    # `components`. A key collision would also land here. The comparison costs
    # nothing and turns "the hash matched" into "these named inputs matched".
    if ! diff -q <(printf '%s\n' "$comp") "$entry/components" >/dev/null 2>&1; then
        echo "reference-cache: MISS — entry $key has a different component block:" >&2
        diff <(printf '%s\n' "$comp") "$entry/components" | sed 's/^/    /' >&2
        return 1
    fi

    # The stored recording must be the one that was stored. A truncated or
    # tampered entry must be refused, not served: this is the one failure the
    # content-addressing above cannot catch, because the key describes the
    # INPUTS and this describes the OUTPUT.
    [ -s "$entry/vanilla.json" ] || { echo "reference-cache: MISS — $key has no recording" >&2; return 1; }
    local want have
    want="$(cat "$entry/recording.sha256" 2>/dev/null || echo none)"
    have="$(sha256sum "$entry/vanilla.json" | cut -d' ' -f1)"
    if [ "$want" != "$have" ]; then
        echo "reference-cache: REFUSING $key — the stored recording does not match its own digest." >&2
        echo "    stored=$want actual=$have" >&2
        echo "    The entry is corrupt or was written into by something else. Not serving it." >&2
        return 1
    fi

    cp -f "$entry/vanilla.json" "$dest" || return 1
    echo "reference-cache: HIT $key"
    echo "    recorded $(cat "$entry/recorded-at" 2>/dev/null || echo '?') in $(cat "$entry/recorded-by" 2>/dev/null || echo '?')"
    echo "    no vanilla boot this run — the reference is a function of the inputs above, not of this branch"
    return 0
}

# Store a fresh recording under its key. Never overwrites a good entry: the
# artefact is content-addressed on its inputs, so an existing entry is already
# the right answer, and rewriting it is how a concurrent gate would serve a
# half-written file.
#
# $1 root  $2 bin  $3 seed  $4 src
refcache_store() {
    local root="$1" bin="$2" seed="$3" src="$4"
    [ -n "${RUSTMC_REFERENCE_CACHE:-}" ] || return 0
    [ -s "$src" ] || { echo "reference-cache: not storing — $src is empty" >&2; return 0; }

    local comp; comp="$(refcache_components "$root" "$bin" "$seed")"
    if printf '%s\n' "$comp" | grep -c '=MISSING$' >/dev/null; then
        echo "reference-cache: not storing — a key input is unavailable" >&2
        return 0
    fi
    local key; key="$(printf '%s\n' "$comp" | sha256sum | sed 's/^/ref:/;s/ .*//')"
    local entry="$REFCACHE_DIR/${key#ref:}"
    [ -d "$entry" ] && { echo "reference-cache: entry $key already present, keeping it"; return 0; }

    # Build the entry complete in a temp dir and rename it into place. A reader
    # either sees no directory or sees a finished one; it never sees a
    # `vanilla.json` whose `recording.sha256` has not been written yet.
    mkdir -p "$REFCACHE_DIR" || return 0
    local tmp; tmp="$(mktemp -d "$REFCACHE_DIR/.staging.XXXXXX")" || return 0
    cp -f "$src" "$tmp/vanilla.json" || { rm -rf "$tmp"; return 0; }
    printf '%s\n' "$comp" > "$tmp/components"
    sha256sum "$tmp/vanilla.json" | cut -d' ' -f1 > "$tmp/recording.sha256"
    date -Is > "$tmp/recorded-at"
    printf '%s\n' "$root" > "$tmp/recorded-by"
    if mv -T "$tmp" "$entry" 2>/dev/null; then
        echo "reference-cache: stored $key"
    else
        # Lost a race with another tree storing the identical artefact. That is
        # the good outcome, not an error: both derived the same key from the
        # same inputs.
        rm -rf "$tmp"
        echo "reference-cache: $key was stored concurrently, keeping theirs"
    fi
    return 0
}
