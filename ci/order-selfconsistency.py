#!/usr/bin/env python3
"""order-selfconsistency.py — for each order-keyed site on the wire, does
vanilla agree with ITSELF about it, measured over the recordings on disk?

# Why this is the hard half

`ci/order-keyed-census.py` says which behaviours are keyed on an order. That
alone justifies no work at all. Vanilla disagreeing with itself is the null
hypothesis in this project — the harvest is non-deterministic in its own
right, and `-Dmax.bg.threads=1` does not clamp it (refuted in the nether,
where the clamped spread was WIDER: 34/438/460 against 75/134/151). So for
every order the census finds, the question that decides whether there is any
work to do is whether two cold vanilla runs produce the same order. If they
do not, matching it is not a requirement and claiming otherwise manufactures
work.

Three verdicts, and the third is the one this tool exists to be able to
return:

  SELF-CONSISTENT     every run agreed. Matching it is a real requirement.
  NOT-SELF-CONSISTENT the runs disagreed. Not a requirement; a check that
                      compared it would flap.
  UNMEASURABLE        the recordings hold no proxy that isolates this order.
                      NOT the same as self-consistent, and reporting it as
                      one is the failure mode this file is shaped against.

# The proxies, and what each of them cannot separate

The recordings do not carry packet order as a field; they carry a sequence of
observations per socket. So each proxy is an argument as well as a
measurement, and the argument is written next to it.

Every count states N. N is the number of DISTINCT recordings, de-duplicated
on the sha256 of the scenario block, because the reference cache serves one
recording to several trees and counting files overcounts.
"""

import argparse
import glob
import hashlib
import json
import os
import sys
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
PIN = os.path.join(HERE, "checks", "order-selfconsistency.json")
# WHERE THE SEARCH LOOKS. Note what this is not: it is not `$ROOT`. Every
# recording on the box is a candidate, which is right — a gate directory holds
# at most one and self-consistency needs at least two — and it is exactly why
# the DECLARED population below exists. What may be FOUND is the box; what may
# ENTER the measurement is a fixed list.
DEFAULT_GLOB = os.path.expanduser("~/rustmc*/oracle-work/gameplay*/vanilla.json")

# ===== THE POPULATION WAS THE WHOLE BOX — repaired 2026-09-05 ================
#
# `--check` runs in `ci/t0.sh` and its input was `sorted(glob.glob(...))` over
# `~/rustmc*/oracle-work/gameplay*/vanilla.json`: **every tree on this box**.
# So any lane that recorded a vanilla run changed this check's population, and
# therefore its verdict rows, on every other branch at once. It reddened T0 on
# unmodified `main` `b6237f29c` (reproduced from `git archive`), and the
# repairs lane hit the same message on `stack-worldgen`.
#
# THE MESSAGE WAS ALSO WRONG ABOUT WHY, which is what made it expensive. The
# comment under the comparison read: "The pin holds verdicts and not counts
# precisely so that another tree recording more runs cannot fire this: it
# fires only when a site that agreed with itself has stopped, or the reverse."
# Measured on main at the moment it was red: **not one pinned verdict had
# moved.** Two scenarios the pin has never held —
# `a-bat-left-far-away-despawns-and-a-pig-does-not` and
# `a-spawned-pig-syncs-its-movement-speed-a-tick-later`, both recorded into
# other lanes' trees — had ENTERED the population, and a set comparison cannot
# tell an added row from a moved one. So the check reported a finding about
# vanilla and what had happened was that somebody else took a recording.
#
# Two things change here and they are separable:
#
#   1. **The population is declared**, by the sha256 of each recording's
#      scenario block — the same digest this file already computes to
#      de-duplicate the reference cache. The search still sweeps the box,
#      because a gate directory has at most one recording of its own and N = 1
#      is the answer this whole file is shaped against; but a recording that
#      is not declared is IGNORED and counted, and a declared one that is
#      missing is a REFUSAL and not a red. Widening the population is then a
#      `--rewrite-pin` in a reviewed commit, which is what it always should
#      have been.
#   2. **Added and removed rows are reported apart from moved ones.** A new
#      corpus scenario reaching the population is a different event from a
#      site that agreed with itself and has stopped, and only the second is a
#      finding about vanilla. They read identically today.
#
# EXIT CODES, and the distinction is the point:
#
#   0  checked, verdicts unchanged
#   1  checked, a verdict MOVED (or a row appeared/vanished) — a branch red
#   2  could not check: declared recordings are not on this box. An
#      environment fault, reported and not read as the branch's, the way
#      `ci/t1-gate.sh` exits 2 on absent extracts
#   3  refused for a METHODOLOGICAL reason — N < 2, no subject, a golden table
#      that no longer covers the corpus. Those are reds: the instrument is
#      saying it cannot measure what it claims to.
POPULATION_KEY = "population"

# ===== AND A DECLARED POPULATION IS ONLY AS DURABLE AS THE BOX — 2026-09-06 ==
#
# Declaring the population by sha256 fixed WHICH recordings count and did not
# fix whether they exist. The recordings live in other lanes' trees, and those
# come and go all night: nine lane directories created and three destroyed in
# four hours on 2026-09-05, and one of the 26 recordings declared that evening
# was already off the box the next morning. So `--check` exited 2 —
# "1 of 26 declared recordings are not on this box" — on the
# figure-provenance lane's tree, which had touched none of it. The box-wide
# glob's flake had become a box-wide *absence* flake.
#
# THE THREE CANDIDATES, AND WHY THE ANSWER IS NOT AN ERROR-HANDLING POLICY.
#
#   (b) ignore a missing declared recording, the way an undeclared one is
#       ignored. Rejected on the merits, not on taste. `walking-away-drops-a-
#       standing-player-from-view` is NOT-SELF-CONSISTENT over 25 runs; lose 24
#       of them and the row drops below two runs, `attr_rows` skips it, and the
#       comparison reports VANISHED — a red for tree churn, which is the flake
#       again in different words. In the other direction
#       `spawn-egg-makes-a-mob` is SELF-CONSISTENT over 25 runs and would stay
#       SELF-CONSISTENT over 2, with `p_hide` gone from 12% to 77.6% and
#       nothing said. That is the single most seductive wrong answer this file
#       exists to refuse, written into the file.
#   (c) error only below a floor on N. It postpones the flake instead of
#       removing it — 24 survives a floor of 20 and so does 21 — and a floor on
#       the total cannot protect a row whose own evidence is two recordings.
#   (a) an error, as now. Right, and the reason is what the recordings ARE.
#
# **The recordings are this check's FIXTURE, not its subject.** Vanilla's
# self-consistency does not depend on our tree; what a branch can actually move
# is the proxy code in this file and the golden walk table in
# `ci/checks/entity-broadcast-order.json`. So this is a golden-vector test whose
# vectors were being read out of whichever lane directories happened to exist,
# and this project's rule for a missing fixture is already written down:
# `crates/rustmc-data/src/fixture.rs` PANICS rather than skipping, and
# `ci/t1-gate.sh` exits 2 rather than measuring. A missing fixture is an error.
# A fixture that can go missing is a bad fixture.
#
# So: (a), and the fixture stops being missable. `--extract` reduces the
# declared population to the two streams the two proxies read and commits them
# as `ci/checks/order-selfconsistency-corpus.json` — 81 KB, the size of
# `gameplay-baseline.toml`. `--check` reads that file and never globs, so no
# lane's directory being created, rsynced over or deleted can reach it.
#
# NOBODY RE-SWEEPS PER GATE, and that is the other half of the answer. A pin
# the gate rewrites cannot fail, which is this project's central defect class;
# re-sweeping per gate would make every run green by construction. Widening the
# population is `--extract` then `--rewrite-pin` in a reviewed commit that says
# what entered, exactly as it is for every other ratchet here.
#
# WHAT THE REDUCTION KEEPS LIVE. Both proxies run unchanged over the extract:
# `attribute_order` reads (who, rest) per `update_attributes`, and
# `within_tick_groups` reads (who, at_ms, entity) per entity-bearing
# observation and does its own bucketing, de-duplication and >= 2 test here.
# What is dropped is observations whose (who, at_ms) bucket holds ONE row,
# which cannot reach two distinct entities under any grouping — 6,870 of 12,128
# rows. That is lossless for the bucket key as written and NOT for a different
# one, so the key is recorded in the corpus and `--check` refuses if this file's
# has moved. Changing the key means re-extracting, which is the correct cost.
CORPUS = os.path.join(HERE, "checks", "order-selfconsistency-corpus.json")
CORPUS_VERSION = 1
# The bucket key `within_tick_groups` uses. If you change that function's
# grouping, change this string in the same edit: the corpus was reduced under
# it and the reduction is only sound for this key.
BUCKET_KEY = "who,at_ms"
WHO_CODES = {0: "actor", 1: "observer"}
WHO_IDS = {v: k for k, v in WHO_CODES.items()}


def die(msg):
    print("order-selfconsistency: REFUSED — %s" % msg, file=sys.stderr)
    sys.exit(3)


# `unavailable()` and exit 2 lived here for one day, 2026-09-05 to 2026-09-06.
# They were the honest answer to "a declared recording is not on this box", and
# they are deleted with the question: `--check` reads the committed corpus and
# never asks the box anything, so no caller can reach them. An exit code a
# script cannot emit is a branch in every caller's error handling that nothing
# will ever take — `ci/t0.sh` had one — and this project has paid for a dead
# branch before.


def scan(pattern):
    """Every distinct recording the box holds, keyed by its scenario digest."""
    files = sorted(glob.glob(pattern))
    if not files:
        die("no vanilla recordings matched %s" % pattern)
    seen = {}
    for f in files:
        try:
            d = json.load(open(f, encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if "scenarios" not in d:
            continue
        h = hashlib.sha256(json.dumps(d["scenarios"], sort_keys=True).encode()).hexdigest()
        seen.setdefault(h, (d, f))
    if not seen:
        die("%d files matched but none held a scenario block" % len(files))
    return seen, len(files)


def load_recordings(pattern):
    """The AD-HOC population: whatever the glob matched.

    Reachable only from `--json` and `--extract`, never from `--check`, which
    reads the committed corpus. A measurement over what a box happens to hold
    is a fine thing to take by hand and is not a thing to gate on.
    """
    seen, nfiles = scan(pattern)
    return [d for d, _f in seen.values()], nfiles, 0


def reduce_recording(d):
    """One recording, reduced to the two streams the two proxies read.

    Compact by construction — `who` interned, no field names per row — because
    this is committed and the full form is 1.9 MB against 81 KB.
    """
    out = {}
    for sc in d["scenarios"]:
        ua = [[WHO_IDS.get(o["who"], o["who"]), o["fields"].get("rest")]
              for o in sc["obs"] if o["kind"] == "update_attributes"]
        rows = []
        for o in sc["obs"]:
            e = o.get("fields", {}).get("entity")
            if isinstance(e, dict) and "$entity" in e:
                rows.append([WHO_IDS.get(o["who"], o["who"]), o["at_ms"], e["$entity"]])
        seen_bucket = Counter((r[0], r[1]) for r in rows)
        ent = [r for r in rows if seen_bucket[(r[0], r[1])] >= 2]
        if ua or ent:
            out[sc["name"]] = {"ua": ua, "ent": ent}
    return out


def inflate(rec):
    """A reduced recording back into the shape the proxies already expect.

    The proxies are not rewritten to read the corpus — they read what they
    always read, so a change to either of them still moves the verdict, which
    is the whole of what this check is for.
    """
    scenarios = []
    for name, sc in sorted(rec["scenarios"].items()):
        obs = []
        for who, rest in sc["ua"]:
            obs.append({"who": WHO_CODES.get(who, who), "step": 0, "at_ms": 0,
                        "kind": "update_attributes",
                        "fields": {"entity": {"$entity": 0}, "rest": rest}})
        for who, at_ms, ent in sc["ent"]:
            obs.append({"who": WHO_CODES.get(who, who), "step": 0, "at_ms": at_ms,
                        "kind": "move_entity_heartbeat",
                        "fields": {"entity": {"$entity": ent}}})
        scenarios.append({"name": name, "obs": obs})
    return {"scenarios": scenarios}


def write_extract(pattern):
    """Sweep the box once and commit what the proxies read.

    Deliberately NOT run by `--check` and NOT run by any gate: a fixture the
    gate regenerates is a fixture the gate cannot fail against.
    """
    seen, nfiles = scan(pattern)
    recs = []
    for h in sorted(seen):
        d, f = seen[h]
        recs.append({"sha256": h, "seen_at": f, "scenarios": reduce_recording(d)})
    doc = {
        "version": CORPUS_VERSION,
        "bucket_key": BUCKET_KEY,
        "note": ("The vanilla recordings ci/order-selfconsistency.py measures, "
                 "reduced to the two streams its two proxies read and committed "
                 "so that no lane directory being created, rsynced over or "
                 "deleted can move a verdict. Regenerate with --extract and "
                 "re-pin with --rewrite-pin, in one reviewed commit that says "
                 "what entered. Never regenerated by a gate."),
        "recordings": recs,
    }
    with open(CORPUS, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, sort_keys=True, separators=(",", ":"))
        fh.write("\n")
    rows = sum(len(sc["ent"]) for r in recs for sc in r["scenarios"].values())
    print("wrote %s — %d recording(s), %d scenario stream(s), %d entity row(s), "
          "%d files matched" % (CORPUS, len(recs),
                                sum(len(r["scenarios"]) for r in recs), rows, nfiles))
    return 0


def load_corpus():
    """The committed fixture, or a refusal. Never globs."""
    if not os.path.exists(CORPUS):
        die("no corpus at %s. It is a TRACKED file, so its absence is a broken "
            "tree and not a fact about this box; rebuild it with --extract in a "
            "tree that has the recordings and commit it." % CORPUS)
    doc = json.load(open(CORPUS, encoding="utf-8"))
    if doc.get("version") != CORPUS_VERSION:
        die("%s is version %r and this checker reads %r"
            % (CORPUS, doc.get("version"), CORPUS_VERSION))
    if doc.get("bucket_key") != BUCKET_KEY:
        die("%s was reduced under bucket key %r and within_tick_groups now "
            "buckets on %r. The reduction dropped every observation whose "
            "bucket held one row, which is lossless for the old key and not "
            "for a new one. Re-extract." % (CORPUS, doc.get("bucket_key"), BUCKET_KEY))
    if not doc.get("recordings"):
        die("%s holds no recordings" % CORPUS)
    return doc


def verify_extract(pattern):
    """Is the committed corpus a faithful reduction of the real recordings?

    THE ANTI-FABRICATION GUARD, and it deliberately does not run in `ci/t0.sh`:
    it needs the recordings, which is the dependency the corpus exists to
    remove from the gate. It is what an orchestrator or a lane runs on a box
    that still has them, and what makes the committed file evidence rather
    than an assertion.

    It reports how many of the corpus's recordings this box can still check,
    and refuses if that is none — a verification over an empty set is the
    reading this whole file is shaped against.
    """
    doc = load_corpus()
    seen, _n = scan(pattern)
    checked = disagreed = 0
    for rec in doc["recordings"]:
        h = rec["sha256"]
        if h not in seen:
            continue
        checked += 1
        want = reduce_recording(seen[h][0])
        if want != rec["scenarios"]:
            disagreed += 1
            only_a = sorted(set(want) - set(rec["scenarios"]))[:3]
            only_b = sorted(set(rec["scenarios"]) - set(want))[:3]
            print("  DISAGREES %s (%s)" % (h[:12], seen[h][1]), file=sys.stderr)
            if only_a:
                print("    in the recording, not the corpus: %s" % ", ".join(only_a),
                      file=sys.stderr)
            if only_b:
                print("    in the corpus, not the recording: %s" % ", ".join(only_b),
                      file=sys.stderr)
    total = len(doc["recordings"])
    if checked == 0:
        die("none of the %d corpus recordings is on this box, so the corpus "
            "could not be verified against anything. That is not a pass."
            % total)
    print("order-selfconsistency: corpus verified — %d of %d recording(s) are on "
          "this box, %d disagree" % (checked, total, disagreed))
    return 1 if disagreed else 0


def declared_population():
    """The sha256 list the pin was taken over, or a refusal."""
    if not os.path.exists(PIN):
        die("no pin at %s; run --rewrite-pin once and commit it" % PIN)
    pin = json.load(open(PIN, encoding="utf-8"))
    pop = pin.get(POPULATION_KEY)
    if not pop or not pop.get("recordings"):
        # A RATCHET, NOT A WARNING. A pin with no declared population is a pin
        # over whatever the box held that minute, which is the defect being
        # repaired; falling back to the glob would keep it alive under a new
        # name.
        die("the pin at %s declares no `population`. A verdict pinned over an "
            "undeclared population is a verdict over whatever recordings this "
            "box happened to hold, which is what made this check red on "
            "unmodified main. Re-declare with --rewrite-pin." % PIN)
    return pin, [r["sha256"] for r in pop["recordings"]]


def eref(o):
    e = o.get("fields", {}).get("entity")
    return e.get("$entity") if isinstance(e, dict) else None


def attribute_order(recs):
    """PROXY for AttributeMap.attributes and AttributeMap.attributesToSync.

    The recorder consumes only the leading varint of an
    `update_attributes` — the entity id — and every remaining byte falls into
    the trailing FNV digest. The attribute snapshots are those bytes, in the
    order `getSyncableAttributes` or `getAttributesToSync` walked them, so
    the digest IS order-sensitive.

    What it cannot separate: an order change from a VALUE change. Two runs
    with different digests disagree about the payload and this proxy cannot
    say which half moved. That is stated in the verdict rather than glossed:
    a NOT-SELF-CONSISTENT here is a sound refutation of "vanilla always
    agrees", and a SELF-CONSISTENT is sound in the other direction, but
    neither attributes the variation.
    """
    per = defaultdict(list)
    for r in recs:
        for s in r["scenarios"]:
            ua = tuple((o["who"], o["fields"].get("rest"))
                       for o in s["obs"] if o["kind"] == "update_attributes")
            if ua:
                per[s["name"]].append(ua)
    return per


def within_tick_groups(recs):
    """PROXY for ChunkMap.entityMap — the per-tick broadcast walk.

    Groups observations by (socket, at_ms) and keeps those naming two or more
    distinct entities. Within one socket, the order two tracked entities'
    packets arrive in is the order `ChunkMap.tick` walked them.

    What it cannot separate, and this is why the verdict below is
    UNMEASURABLE rather than a number: packets emitted DURING the entity tick
    — `add_entity`, sounds, anything a tick body sends synchronously — leave
    in `EntityTickList` order, which is INSERTION, while `ChunkMap.tick`'s
    own broadcast leaves in hash order. A group mixing the two is a mixture
    of two different orders and no single candidate can explain it. A
    player's own echo does not come from `ChunkMap.tick` at all.
    """
    groups = []
    for r in recs:
        for s in r["scenarios"]:
            buck = defaultdict(list)
            for o in s["obs"]:
                v = eref(o)
                if v is not None:
                    buck[(o["who"], o["at_ms"])].append(v)
            for (who, _ms), vs in buck.items():
                seq = []
                for v in vs:
                    if v not in seq:
                        seq.append(v)
                if len(seq) >= 2:
                    groups.append((s["name"], who, tuple(seq)))
    return groups


def golden_walks():
    p = os.path.join(HERE, "checks", "entity-broadcast-order.json")
    if not os.path.exists(p):
        die("the golden walk table is missing (%s); run "
            "ci/entity-broadcast-order.py --measure" % p)
    d = json.load(open(p, encoding="utf-8"))
    return {tuple(int(x) for x in k.split()): tuple(v["open"])
            for k, v in d["golden"].items()}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recordings", default=None,
                    help="ad-hoc glob; overrides the declared population and "
                         "is refused with --check")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--rewrite-pin", action="store_true")
    ap.add_argument("--extract", action="store_true",
                    help="sweep the box and rewrite the committed corpus")
    ap.add_argument("--verify-extract", action="store_true",
                    help="re-derive the corpus from the recordings this box "
                         "still holds and report any row that disagrees")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    pattern = args.recordings or DEFAULT_GLOB
    if args.extract:
        return write_extract(pattern)
    if args.verify_extract:
        return verify_extract(pattern)

    if args.check and args.recordings:
        # Restored after it was lost in the corpus rewrite and three falsifier
        # arms caught it. Without this a gate could be pointed at any set of
        # files, and the pin would be checked against a population nobody
        # declared — the defect this whole repair closes, reachable by a flag.
        die("--check reads the committed corpus and cannot be given "
            "--recordings. Use --json for an ad-hoc measurement, or --extract "
            "to re-declare the corpus in a reviewed commit.")

    pin = None
    ignored = 0
    if args.check or args.rewrite_pin:
        # THE FIXTURE, NOT THE BOX. Neither of these globs: `--check` must give
        # the same answer in a gate directory at 03:00 as in a worktree at
        # noon, and `--rewrite-pin` must pin over exactly what `--check` will
        # read or the two describe different populations.
        doc = load_corpus()
        recs = [inflate(r) for r in doc["recordings"]]
        nfiles = len(recs)
        if args.check:
            pin, declared = declared_population()
            have = [r["sha256"] for r in doc["recordings"]]
            if sorted(declared) != sorted(have):
                die("the pin declares %d recording(s) and %s carries %d; they "
                    "are two tracked files describing one population and they "
                    "disagree, so neither can be trusted. Re-run --extract then "
                    "--rewrite-pin." % (len(declared), os.path.basename(CORPUS),
                                        len(have)))
    else:
        recs, nfiles, ignored = load_recordings(pattern)
    N = len(recs)
    if N < 2:
        die("N = %d distinct recordings; self-consistency needs at least two "
            "and a one-run answer would read as agreement" % N)

    findings = []

    # --- AttributeMap: update_attributes payload -----------------------------
    per = attribute_order(recs)
    if not per:
        die("no scenario emitted update_attributes; the attribute-order "
            "proxy has no subject and must refuse rather than report "
            "agreement over an empty set")
    attr_rows = []
    for name, runs in sorted(per.items()):
        if len(runs) < 2:
            continue
        attr_rows.append({"scenario": name, "runs": len(runs),
                          "distinct": len(set(runs)),
                          "observations_per_run": sorted({len(x) for x in runs})})
    if not attr_rows:
        die("every update_attributes scenario appears in fewer than two "
            "recordings; nothing can be said about self-consistency")
    for row in attr_rows:
        row["verdict"] = ("SELF-CONSISTENT" if row["distinct"] == 1
                          else "NOT-SELF-CONSISTENT")
    findings.append({"site": "AttributeMap.attributes + attributesToSync",
                     "proxy": "the update_attributes trailing digest",
                     "N": N, "rows": attr_rows})

    # --- ChunkMap.entityMap: the per-tick broadcast walk ---------------------
    walks = golden_walks()
    groups = within_tick_groups(recs)
    if not groups:
        die("no within-tick group names two entities; the broadcast-order "
            "proxy has no subject and must refuse")
    tested = agree_hash = agree_asc = 0
    for _name, _who, seq in groups:
        w = walks.get(tuple(sorted(seq)))
        if w is None:
            continue
        tested += 1
        if seq == w:
            agree_hash += 1
        if seq == tuple(sorted(seq)):
            agree_asc += 1
    if tested == 0:
        die("no within-tick group's id set has a golden walk; widen ID_SETS "
            "in ci/entity-broadcast-order.py rather than reporting nothing")
    findings.append({
        "site": "ChunkMap.entityMap",
        "proxy": "same socket, same at_ms, two or more distinct entities",
        "N": N, "groups": len(groups), "tested": tested,
        "agree_with_hash_walk": agree_hash,
        "agree_with_ascending_id": agree_asc,
        "verdict": "UNMEASURABLE",
        "why": ("neither candidate order explains the data — %d of %d for the "
                "hash walk against %d for ascending id — because a group mixes "
                "packets emitted during the entity tick, which leave in "
                "EntityTickList INSERTION order, with ChunkMap.tick's own "
                "broadcast, which leaves in hash order. The proxy cannot "
                "separate them, so this is a derivation with no corpus verdict "
                "and must not be reported as one."
                % (agree_hash, tested, agree_asc))})

    # --- the insertion-ordered sites ----------------------------------------
    findings.append({
        "site": "EntityTickList.active + EntityLookup.byId + "
                "ClassInstanceMultiMap.allInstances",
        "proxy": None, "N": N, "verdict": "UNMEASURABLE",
        "why": ("all three are INSERTION-ordered, and the recordings carry no "
                "proxy that isolates an insertion order from the hash-ordered "
                "broadcast that follows it. Recorded as unmeasured rather "
                "than as agreement.")})

    # --- the one already measured elsewhere ---------------------------------
    findings.append({
        "site": "ServerChunkCache.chunkHoldersToBroadcast",
        "proxy": "measured on a prior branch, not re-derived here",
        "N": None, "verdict": "NOT-SELF-CONSISTENT",
        "why": ("4.24% self-agreement over four chunks against 1/4! = 4.17%, "
                "which is n! near-uniform. `ci/idhash-determinism.sh` on this "
                "branch says the cause is NOT that identity hashing is random: "
                "it is that which chunk is first marked dirty varies per run, "
                "and the set only exposes it.")})

    report = {"N_distinct_recordings": N, "files_matched": nfiles,
              "findings": findings}

    if args.rewrite_pin:
        # The pin holds the VERDICTS and N, not the raw counts: the counts
        # move whenever a tree records another run, and pinning them would
        # make this check a tripwire on the reference cache instead of on the
        # findings.
        # The population travels WITH the verdicts, in the same file and the
        # same commit. Declaring it anywhere else would let the two drift, and
        # a verdict whose population is unknown is the thing being repaired.
        doc = load_corpus()
        by_digest = {r["sha256"]: r.get("seen_at", "unknown") for r in doc["recordings"]}
        used = [r["sha256"] for r in doc["recordings"]]
        pinned = {"findings": [{"site": f["site"],
                                "verdict": f.get("verdict"),
                                "rows": [{"scenario": r["scenario"],
                                          "verdict": r["verdict"]}
                                         for r in f.get("rows", [])]}
                               for f in findings],
                  POPULATION_KEY: {
                      "note": ("the recordings these verdicts were measured "
                               "over, by the sha256 of the scenario block. "
                               "`seen_at` is where each one was on the day it "
                               "was declared and is a hint for restoring it, "
                               "not an identity: the digest is the identity."),
                      "recordings": [{"sha256": h,
                                      "seen_at": by_digest.get(h, "unknown")}
                                     for h in sorted(used)]}}
        with open(PIN, "w", encoding="utf-8") as fh:
            json.dump(pinned, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("rewrote %s" % PIN)
        return 0

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0

    print("is vanilla self-consistent about each order?  N = %d distinct "
          "recordings (%d files)" % (N, nfiles))
    print()
    for f in findings:
        print("  %s" % f["site"])
        if f.get("proxy"):
            print("    proxy: %s" % f["proxy"])
        if "rows" in f:
            for r in f["rows"]:
                print("      %-22s runs=%-3d distinct=%-3d obs/run=%-8s %s"
                      % (r["scenario"][:22], r["runs"], r["distinct"],
                         ",".join(map(str, r["observations_per_run"])), r["verdict"]))
        else:
            print("    verdict: %s" % f["verdict"])
            if "tested" in f:
                print("      %d groups, %d with a golden walk; hash walk %d, "
                      "ascending %d" % (f["groups"], f["tested"],
                                        f["agree_with_hash_walk"],
                                        f["agree_with_ascending_id"]))
            print("      %s" % f["why"])
        print()

    if args.check:
        want = {"findings": pin["findings"]}
        got = {"findings": [{"site": f["site"], "verdict": f.get("verdict"),
                             "rows": [{"scenario": r["scenario"],
                                       "verdict": r["verdict"]}
                                      for r in f.get("rows", [])]}
                            for f in findings]}
        if want != got:
            # THREE EVENTS, NOT ONE, and the old message called all of them "a
            # verdict moved". Only MOVED is a finding about vanilla; APPEARED
            # and VANISHED are facts about which scenarios the population's
            # recordings carry. Conflating them cost the merge chain a red on
            # unmodified main, where nothing had moved and two rows had
            # appeared.
            def flat(doc):
                out = {}
                for f in doc["findings"]:
                    rows = f.get("rows") or []
                    if rows:
                        for r in rows:
                            out[(f["site"], r["scenario"])] = r["verdict"]
                    else:
                        out[(f["site"], None)] = f.get("verdict")
                return out
            a, b = flat(want), flat(got)
            moved = [(k, a[k], b[k]) for k in sorted(set(a) & set(b), key=str)
                     if a[k] != b[k]]
            appeared = sorted(set(b) - set(a), key=str)
            vanished = sorted(set(a) - set(b), key=str)
            if moved:
                print("order-selfconsistency: a verdict MOVED against its pin. "
                      "This is a finding about vanilla, not a stale figure — a "
                      "site that agreed with itself over N runs has stopped, or "
                      "one that did not has started. Read the new N, say which, "
                      "and update PARITY-ROADMAP.md §101.6 in the same commit "
                      "as the pin.", file=sys.stderr)
                for (site, scen), was, now in moved:
                    print("  MOVED    %s / %s: %s -> %s"
                          % (site, scen, was, now), file=sys.stderr)
            if appeared or vanished:
                print("order-selfconsistency: the pin's ROW SET no longer "
                      "matches, which is NOT the same event as a verdict "
                      "moving. The declared population's recordings carry "
                      "scenarios the pin does not name, or no longer carry "
                      "ones it does. Re-declare with --rewrite-pin in a "
                      "reviewed commit and say what entered.", file=sys.stderr)
                for k in appeared:
                    print("  APPEARED %s / %s = %s" % (k[0], k[1], b[k]),
                          file=sys.stderr)
                for k in vanished:
                    print("  VANISHED %s / %s (was %s)" % (k[0], k[1], a[k]),
                          file=sys.stderr)
            return 1
        print("order-selfconsistency: %d sites, verdicts unchanged, N = %d "
              "declared recording(s), %d on this box ignored as undeclared, "
              "%d files matched" % (len(findings), N, ignored, nfiles))
    return 0


if __name__ == "__main__":
    sys.exit(main())
