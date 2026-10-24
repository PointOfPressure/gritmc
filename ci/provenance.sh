#!/usr/bin/env bash
# provenance.sh — sourced helper. Stamps and checks the generated tables under
# `data/vanilla/`, so a gate can tell a table this tree generated from one that
# arrived from somewhere else.
#
# WHY. `data/vanilla/` is gitignored, excluded from `ci/t0-remote.sh`'s rsync,
# and on the Threadripper it is populated by hand — historically with
# `cp -rn ~/rustmc/data/vanilla ~/$RUSTMC_REMOTE_DIR/data/`. `cp -rn` is
# **no-clobber**, so once a gate directory has a table it keeps that table
# forever, and a table a branch has not yet invented never arrives at all.
#
# That cost a whole gate on 2026-08-03. `~/rustmc-gate` was gating
# `placed-by-and-ticks` with `data/vanilla/set_placed_by.json` **absent** (the
# branch's central table, which does not exist on main) and
# `use_without_item.json` **stale by 111 KB** (the branch rewrote
# `UseGolden.java` to carry setBlock flags and tick delays). rustmc therefore
# behaved as if half the branch were not there — `powered=false` where vanilla
# had `true`, one `block_update` where vanilla sent both halves of a bed — and
# the gate reported 12/55 with 11 regressed. It was not a red branch. It was a
# **check that could not pass**, which is this project's oldest failure mode
# wearing a measurement's costume.
#
# Neither half of that would have been caught by asking "is the file there".
# One file was there and wrong. So a stamp records three things and any of them
# disagreeing is a refusal:
#
#   host=     a stable id for the machine that generated it. See below — this
#             is the half `tree=` cannot do, and its absence was a live hole.
#   tree=     the absolute path of the tree that generated it. A table copied
#             in from another checkout names that checkout and is refused.
#   md5=      the table's own digest at stamp time. A table replaced after
#             stamping no longer matches its stamp.
#   gen_md5=  the digest of the generator — the `ci/*.sh` script **and** every
#             `tools/goldengen/vanilla/*.java` it names. This is the half that
#             catches same-tree staleness: when a branch changes `UseGolden.java`
#             the old `use_without_item.json` stops matching its own stamp's
#             generator digest, and the gate says so instead of measuring a
#             server that is missing the data the branch is about.
#
# `commit=` is recorded but deliberately NOT required to match: a table does not
# go stale because an unrelated commit landed, and a gate that demands it would
# regenerate 14 MB of block properties on every push and then be switched off.
#
# # WHY `tree=` IS NOT ENOUGH, AND WHY THAT WAS A HOLE RATHER THAN A GAP
#
# `tree=` is an **absolute path**, and the only test on it is string equality.
# The laptop and the Threadripper run the same user with the same home, so
# `/home/sayer/rustmc` names a different checkout on a different machine on
# each of them and the two strings are byte-identical. A `data/vanilla` built
# on the laptop and copied to the box therefore satisfied every clause of
# `prov_check` — right path, matching md5, matching generator digest — while
# carrying tables no build on that box had ever produced.
#
# That is exactly the failure this file was written to stop, one layer up: the
# whole point of the stamp is that a gate must never measure a table it did not
# make, and "the path matches" answers a question about the filesystem when the
# question is about provenance. The convention that makes it likely rather than
# theoretical is the project's own: every agent is told to work in
# `~/<remote-dir>` on the box, mirroring `~/<dir>` on the laptop, and the two
# canonical checkouts are both `~/rustmc`.
#
# So the stamp names the machine, and `prov_check` requires it. `host=` is
# `<hostname>/<source>:<id>` where the id is `/etc/machine-id` — assigned once
# at install, stable across reboots and renames, and different on every
# machine. The source is written into the field so an id that came from a
# fallback can never be mistaken for one that did not, and the hostname is
# there so a refusal reads as "generated on the laptop" rather than as a hex
# string. A stamp written before this existed has no `host=` line at all and is
# refused, which costs one regeneration per gate directory and is the safe
# direction: the table it would otherwise bless is one whose origin is unknown.
#
# Sourced, never executed:  . "$ROOT/ci/provenance.sh"

# Where a stamp lives for a given table.  data/vanilla/x.json -> data/vanilla/.provenance/x.json.prov
prov_path() {
    local f="$1"
    printf '%s/.provenance/%s.prov' "$(dirname "$f")" "$(basename "$f")"
}

# Digest of a generator: the script, plus every goldengen java source whose
# class name it mentions. Named sources only — hashing the whole directory
# would invalidate the item table because a structure dumper changed.
#
# LATENT, MARKED 2026-08-06, NOT FIXED HERE. **This digest does not cover the
# game version, and every one of these tables is a function of it.** The
# generators read `GAME_VERSION` out of `crates/rustmc-proto/src/lib.rs` and dump
# from `oracle-work/vanilla-$VER`; bump that constant and each generator starts
# reading a different jar, but the script and its java are untouched, so
# `gen_md5` is unchanged, `md5=` still matches the table nobody regenerated, and
# `prov_check` returns 0. Every table then silently belongs to the previous
# version while the stamp asserts it is current.
#
# That is the same shape as the staleness this file was written to catch —
# `use_without_item.json` 111 KB behind its own dumper — with the input moved
# one level out of view. It is the sharper case, because the whole point of a
# stamp is that a gate must never measure a table it did not make, and this one
# blesses a table made from a different jar.
#
# Unfired so far only because the version has not moved since stamps existed.
# The fix is to fold `GAME_VERSION` into the digest; it is not this branch's
# item and is written here rather than in a doc so it is read by whoever next
# touches the digest.
prov_gen_digest() {
    local root="$1" script="$2"
    {
        md5sum "$root/$script" 2>/dev/null
        if [ -d "$root/tools/goldengen/vanilla" ]; then
            local j
            for j in "$root"/tools/goldengen/vanilla/*.java; do
                [ -e "$j" ] || continue
                if grep -q "$(basename "$j" .java)" "$root/$script" 2>/dev/null; then
                    md5sum "$j"
                fi
            done
        fi
    } | sort -k2 | md5sum | cut -d' ' -f1
}

# A stable identity for this machine: `<hostname>/<source>:<id>`.
#
# `/etc/machine-id` is written once at install and survives reboots, renames
# and clones of the home directory, which is the property wanted — a hostname
# alone is mutable and two boxes can carry the same one. The source is part of
# the value so a fallback id can never compare equal to a real one: a machine
# that grows an `/etc/machine-id` after a stamp was written is a machine whose
# stamps must be re-made, not one whose stamps quietly still pass.
prov_host_id() {
    local id=""
    if [ -r /etc/machine-id ]; then id="$(cat /etc/machine-id 2>/dev/null)"; fi
    if [ -n "$id" ]; then
        printf '%s/machine-id:%s' "$(hostname)" "$id"
        return
    fi
    if [ -r /var/lib/dbus/machine-id ]; then id="$(cat /var/lib/dbus/machine-id 2>/dev/null)"; fi
    if [ -n "$id" ]; then
        printf '%s/dbus-machine-id:%s' "$(hostname)" "$id"
        return
    fi
    printf '%s/hostname-only' "$(hostname)"
}

# prov_stamp <root> <table-path> <generator-script-relative-to-root>
prov_stamp() {
    local root="$1" f="$2" script="$3"
    local p; p="$(prov_path "$f")"
    mkdir -p "$(dirname "$p")"
    {
        printf 'host=%s\n' "$(prov_host_id)"
        printf 'tree=%s\n' "$root"
        printf 'commit=%s\n' "$(cd "$root" && git rev-parse HEAD 2>/dev/null || echo unknown)"
        printf 'generator=%s\n' "$script"
        printf 'generated=%s\n' "$(date -Is)"
        printf 'md5=%s\n' "$(md5sum "$f" | cut -d' ' -f1)"
        printf 'gen_md5=%s\n' "$(prov_gen_digest "$root" "$script")"
    } > "$p"
}

# prov_check <root> <table-path>
# Prints the reason to stdout and returns non-zero when the table may not be
# trusted. Prints nothing and returns 0 when it may.
prov_check() {
    local root="$1" f="$2"
    [ -e "$f" ] || { echo "absent"; return 1; }
    [ -s "$f" ] || { echo "present but empty"; return 1; }
    local p; p="$(prov_path "$f")"
    [ -s "$p" ] || { echo "no provenance stamp — this tree has never generated it"; return 1; }

    local h t c m g here
    h="$(sed -n 's/^host=//p'    "$p")"
    t="$(sed -n 's/^tree=//p'    "$p")"
    c="$(sed -n 's/^generator=//p' "$p")"
    m="$(sed -n 's/^md5=//p'     "$p")"
    g="$(sed -n 's/^gen_md5=//p' "$p")"

    # The machine, before the path. `tree=` cannot tell two machines apart when
    # both have the same home — see the header — so a table that passes the
    # path test and fails this one is precisely the case that used to slip
    # through, and the message has to say so rather than saying "foreign".
    here="$(prov_host_id)"
    [ -n "$h" ] || { echo "stamp predates the host binding — regenerate"; return 1; }
    [ "$h" = "$here" ] || {
        echo "FOREIGN MACHINE — generated on $h, this is $here"; return 1; }
    [ "$t" = "$root" ] || { echo "FOREIGN — generated in $t, not in $root"; return 1; }
    [ "$m" = "$(md5sum "$f" | cut -d' ' -f1)" ] || { echo "changed since it was stamped"; return 1; }
    [ -n "$c" ] || { echo "stamp names no generator"; return 1; }
    [ "$g" = "$(prov_gen_digest "$root" "$c")" ] || {
        echo "STALE — $c has changed since this table was generated"; return 1; }
    return 0
}
