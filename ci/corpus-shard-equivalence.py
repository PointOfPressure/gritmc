#!/usr/bin/env python3
"""corpus-shard-equivalence.py — does sharding a corpus run change what it measures?

    ci/corpus-shard-equivalence.py compare --bin target/release/oracle-runner \\
        --checks ci/checks/gameplay --serial A/ --control B/ [--control C/ ...] --sharded S/ [--json out.json]
    ci/corpus-shard-equivalence.py --selftest --bin target/release/oracle-runner
    ci/corpus-shard-equivalence.py --check

Each run directory holds the three files `ci/t2-gameplay.sh` leaves in
`oracle-work/gameplay/` under `T2_GAMEPLAY_KEEP=1`: `vanilla.json`,
`rustmc.json` and `report.json`. `A` and `B` are two SERIAL runs
(`RUSTMC_CORPUS_SHARDS=1`) and `S` is a sharded one, all of the same scenario
set in the same tree.

# What "equivalent" means, and why it has a control

`RUSTMC_CORPUS_SHARDS=N` records the corpus as N contiguous runs, each starting
at the first workspace on a fresh connection. That moves three things a serial
run holds fixed: which workspace (absolute x) a scenario runs at, which
scenarios ran before it on the same connection, and the per-connection
counters. `compare` already rebases the counters and 32-block workspaces keep
every section-relative coordinate, so the expected answer is "nothing" — and
an expected answer is exactly what this project refuses to take on faith.

Byte identity of the raw recordings is the wrong bar: two SERIAL runs are not
byte-identical either (`at_ms` is wall clock, and vanilla has a floor —
`docs/PARITY.md` §4). So the bar is the recorder's own projection, applied in
both directions (reference and subject swapped), per side:

  * **subject (rustmc) projection** — sharded against serial. Any scenario
    that disagrees here and does NOT disagree between the two serial runs is a
    shard effect.
  * **reference (vanilla) projection** — the same, with the serial-vs-serial
    control standing in for vanilla's own floor.
  * **verdicts** — each run's `report.json` outcome (`verdict`, `strict`).
    A scenario whose verdict differs between S and A and not between B and A
    is a shard effect.

The control is what makes this a measurement rather than a hope: a scenario
that flaps between two serial runs cannot be charged to sharding, and a
scenario that flaps ONLY under sharding cannot be excused as vanilla noise.

EXIT: 0 equivalent · 1 a shard effect was found (RED) · 2 refused (the inputs
are not three recordings of one scenario set).
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

AGREE = {"match", "vacuous", "mute", "inert"}
LEDGER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "checks",
                      "corpus-shard-equivalence.toml")


def refuse(msg):
    print(f"SHARD-EQUIVALENCE REFUSED — {msg}", file=sys.stderr)
    raise SystemExit(2)


def load(d, f):
    p = os.path.join(d, f)
    try:
        with open(p) as fh:
            return json.load(fh)
    except (OSError, ValueError) as e:
        refuse(f"{p}: {e}")


def names(trace):
    return [s["name"] for s in trace.get("scenarios", [])]


def outcomes(report):
    return {o["name"]: (o.get("verdict"), o.get("strict")) for o in report.get("outcomes", [])}


def self_compare(binary, checks, ref, sub, tmp, tag):
    """Scenario -> (verdict, strict) for `compare --reference ref --subject sub`."""
    out = os.path.join(tmp, f"{tag}.json")
    r = subprocess.run([binary, "compare", checks, "--reference", ref, "--subject", sub,
                        "--json", out], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                       text=True)
    if not os.path.exists(out):
        refuse(f"`{binary} compare` wrote no report for {tag} (rc {r.returncode}): "
               f"{r.stderr.strip()[-300:]}")
    return outcomes(json.load(open(out)))


def disagreements(binary, checks, a, b, who, tmp):
    """Scenarios whose `who` recording differs between runs a and b, both directions."""
    ra, rb = os.path.join(a, f"{who}.json"), os.path.join(b, f"{who}.json")
    fwd = self_compare(binary, checks, ra, rb, tmp, f"{who}-fwd")
    back = self_compare(binary, checks, rb, ra, tmp, f"{who}-back")
    bad = set()
    for n in set(fwd) | set(back):
        for v in (fwd.get(n), back.get(n)):
            if v is None or v[0] not in AGREE or v[1] not in AGREE:
                bad.add(n)
    return bad


def fresh_first(trace):
    return [s["name"] for s in trace["scenarios"] if s.get("fresh_session")]


def known_unstable(root=None):
    """Scenario names the corpus already declares vanilla cannot reproduce:
    `[[unstable]]` in gameplay-unstable.toml and `[[evidence]]` in
    gameplay-floor.toml. One serial control is N=2; a scenario whose own
    floor is known is not charged to sharding for flapping once more."""
    import tomllib
    root = root or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = set()
    for f, key in (("gameplay-unstable.toml", "unstable"), ("gameplay-floor.toml", "evidence")):
        try:
            doc = tomllib.load(open(os.path.join(root, "ci", "checks", f), "rb"))
        except OSError:
            continue
        out |= {e["name"] for e in doc.get(key, []) if "name" in e}
    return out


def compare(binary, checks, serial, control, sharded, json_out=None, known=frozenset()):
    # `control` is one serial run or a list of them. Every control is another
    # serial run compared against the SAME serial run, and a scenario that
    # disagrees in ANY of them is the floor: N serial runs give N-1 chances to
    # see a flap, and a single control (N=2) left the 080-089 mob cluster's
    # verdict flips unbounded (2026-10-01).
    controls = [c for c in (control if isinstance(control, (list, tuple)) else [control]) if c]
    control = controls[0] if controls else None
    runs = {"serial": serial, "sharded": sharded}
    for k, c in enumerate(controls):
        runs["control" if k == 0 else f"control{k + 1}"] = c
    order = None
    for tag, d in runs.items():
        for who in ("vanilla", "rustmc"):
            n = names(load(d, f"{who}.json"))
            if order is None:
                order = n
            elif n != order:
                refuse(f"{tag}/{who}.json covers a different scenario list "
                       f"({len(n)} vs {len(order)}, order-equal={sorted(n) == sorted(order)}) "
                       "— these are not recordings of one scenario set")
    if not order:
        refuse("no scenarios")
    for tag, d in runs.items():
        rep = outcomes(load(d, "report.json"))
        if sorted(rep) != sorted(order):
            refuse(f"{tag}/report.json does not report the recorded scenarios")

    tmp = tempfile.mkdtemp(prefix="shard-eq.")
    try:
        res = {}
        for who in ("rustmc", "vanilla"):
            res[who] = {"sharded": disagreements(binary, checks, serial, sharded, who, tmp)}
            res[who]["control"] = set()
            for c in controls:
                res[who]["control"] |= disagreements(binary, checks, serial, c, who, tmp)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    va, vs = outcomes(load(serial, "report.json")), outcomes(load(sharded, "report.json"))
    vcs = [outcomes(load(c, "report.json")) for c in controls]
    flips = {n for n in order if va[n] != vs[n]}
    flips_ctl = {n for n in order for vc in vcs if va[n] != vc[n]}

    raw = {
        "rustmc": res["rustmc"]["sharded"] - res["rustmc"]["control"],
        "vanilla": res["vanilla"]["sharded"] - res["vanilla"]["control"],
        "verdict": flips - flips_ctl,
    }
    # Declared-unstable scenarios are excused on the VANILLA and VERDICT sides
    # only: they are unstable because vanilla is. The subject gets no excuse.
    shard_effect = {
        "rustmc": sorted(raw["rustmc"]),
        "vanilla": sorted(raw["vanilla"] - set(known)),
        "verdict": sorted(raw["verdict"] - set(known)),
    }
    excused = sorted((raw["vanilla"] | raw["verdict"]) & set(known))
    tr = load(sharded, "rustmc.json")
    # The first shard is the serial run's own prefix: same predecessors, same
    # workspaces, and it starts on a fresh session exactly as the serial run
    # does. Sharding changes nothing for it but the load of the other shards
    # running beside it. So an effect charged there cannot be a predecessor
    # effect; it is run-to-run noise the N=2 control missed, or load. Reported,
    # never subtracted: the verdict below does not read it.
    starts = [i for i, s in enumerate(tr["scenarios"]) if s.get("fresh_session")]
    first_end = starts[1] if len(starts) > 1 else len(order)
    first_shard = set(order[:first_end])
    effected = set(shard_effect["rustmc"]) | set(shard_effect["vanilla"]) | set(shard_effect["verdict"])
    in_first = sorted(effected & first_shard)
    summary = {
        "scenarios": len(order),
        "shard_starts": fresh_first(tr),
        "rustmc": {"sharded_vs_serial": sorted(res["rustmc"]["sharded"]),
                   "control_vs_serial": sorted(res["rustmc"]["control"])},
        "vanilla": {"sharded_vs_serial": sorted(res["vanilla"]["sharded"]),
                    "control_vs_serial": sorted(res["vanilla"]["control"])},
        "verdict_flips": {"sharded_vs_serial": {n: [va[n], vs[n]] for n in sorted(flips)},
                          "control_vs_serial": {n: [va[n]] + [vc[n] for vc in vcs]
                                                for n in sorted(flips_ctl)}},
        "controls": len(controls),
        "shard_effect": shard_effect,
        "excused_known_unstable": excused,
        "effects_in_first_shard": in_first,
        "controlled": bool(controls),
    }
    print(f"scenarios: {len(order)}; shard starts (fresh connection): {len(summary['shard_starts'])}")
    for who in ("rustmc", "vanilla"):
        print(f"{who:8s} projection: sharded-vs-serial disagree on {len(res[who]['sharded'])}, "
              f"serial-vs-serial on {len(res[who]['control'])} over {len(controls)} control(s)"
              + ("" if control else " (NO CONTROL)"))
    print(f"verdicts: sharded-vs-serial flip {len(flips)}, serial-vs-serial flip {len(flips_ctl)}")
    for n in excused:
        print(f"  excused (declared vanilla-unstable) {n}")
    if effected:
        print(f"  {len(in_first)} of {len(effected)} shard-effect scenario(s) are in the first shard, "
              "whose predecessors and coordinates are the serial run's own — noise or load, not history")
    red = any(shard_effect.values())
    for k, v in shard_effect.items():
        for n in v:
            print(f"  SHARD EFFECT [{k}] {n}")
    if json_out:
        with open(json_out, "w") as fh:
            json.dump(summary, fh, indent=2)
    print("SHARD-EQUIVALENCE " + ("RED" if red else "EQUIVALENT"))
    return 1 if red else 0


# --- the self-test: every refusal above, injected ---------------------------

SCEN_TOML = """name = "{name}"
about = "selftest scenario {name}"
observe = ["block_update"]
[[step]]
do = "dig"
at = [1, -1, 0]
face = "up"
"""


def _trace(names_, state="minecraft:air", fresh=None):
    sc = []
    for i, n in enumerate(names_):
        sc.append({"name": n, "anchor": [-32 * (i + 1), -60, 0], "spawn": [0.5, -60.0, 0.5],
                   "fresh_session": (fresh or {}).get(n, i == 0),
                   "obs": [{"who": "observer", "step": 0, "at_ms": 25, "kind": "block_update",
                            "fields": {"at": [1, -1, 0], "state": state, "state_id": 0}}]})
    return {"target": "127.0.0.1:0", "version": "26.2", "protocol": 0,
            "recorder": "selftest", "scenarios": sc}


def _report(names_, verdicts=None):
    return {"outcomes": [{"name": n, "verdict": (verdicts or {}).get(n, "match"),
                          "strict": (verdicts or {}).get(n, "match")} for n in names_]}


def selftest(binary):
    if not binary or not os.access(binary, os.X_OK):
        refuse(f"--selftest needs --bin <oracle-runner>; got {binary!r}")
    root = tempfile.mkdtemp(prefix="shard-eq-selftest.")
    ok = bad = 0
    try:
        checks = os.path.join(root, "checks")
        os.makedirs(checks)
        N = ["a-one", "b-two", "c-three", "d-four"]
        for n in N:
            open(os.path.join(checks, f"{n}.toml"), "w").write(SCEN_TOML.format(name=n))

        def run(tag, vtrace, rtrace, report):
            d = os.path.join(root, tag)
            os.makedirs(d, exist_ok=True)
            json.dump(vtrace, open(os.path.join(d, "vanilla.json"), "w"))
            json.dump(rtrace, open(os.path.join(d, "rustmc.json"), "w"))
            json.dump(report, open(os.path.join(d, "report.json"), "w"))
            return d

        base = lambda: (_trace(N), _trace(N), _report(N))
        A = run("A", *base())
        B = run("B", *base())

        def case(label, want, sharded, control=B, name=None, known=frozenset()):
            nonlocal ok, bad
            try:
                import io
                so, se = sys.stdout, sys.stderr
                sys.stdout = sys.stderr = cap = io.StringIO()
                try:
                    rc = compare(binary, checks, A, control, sharded, known=known)
                finally:
                    sys.stdout, sys.stderr = so, se
            except SystemExit as e:
                rc = e.code
            if rc == want and (want != 1 or f"] {name}\n" in cap.getvalue()):
                ok += 1
                print(f"  ok    {label} (exit {rc})")
            else:
                bad += 1
                print(f"  FAIL  {label}: exit {rc}, wanted {want}")

        # 1. positive control: a sharded run that is the serial run, with the
        #    fresh-connection flags a 2-shard layout writes, is equivalent.
        case("identical recordings, shard-boundary fresh flags -> equivalent", 0,
             run("S1", _trace(N, fresh={"a-one": True, "c-three": True}),
                 _trace(N, fresh={"a-one": True, "c-three": True}), _report(N)))
        # 2. the subject's projection moved on one scenario under sharding only.
        rt = _trace(N)
        rt["scenarios"][2]["obs"][0]["fields"]["state"] = "minecraft:stone"
        case("subject projection moved on one scenario -> RED", 1,
             run("S2", _trace(N), rt, _report(N)), name="c-three")
        # 3. same on the reference side.
        vt = _trace(N)
        vt["scenarios"][1]["obs"] = []
        case("reference went silent on one scenario -> RED", 1,
             run("S3", vt, _trace(N), _report(N)), name="b-two")
        # 4. a verdict flipped under sharding.
        case("a verdict flipped under sharding -> RED", 1,
             run("S4", _trace(N), _trace(N), _report(N, {"d-four": "diverge"})), name="d-four")
        # 5. ...but the same flip in the serial-vs-serial control is the floor.
        Bf = run("Bf", _trace(N), _trace(N), _report(N, {"d-four": "diverge"}))
        case("the same flip present in the serial control -> not charged", 0,
             run("S5", _trace(N), _trace(N), _report(N, {"d-four": "diverge"})), control=Bf)
        # 6. a reference disagreement present in the control too is the floor.
        vtc = _trace(N)
        vtc["scenarios"][1]["obs"] = []
        Bv = run("Bv", vtc, _trace(N), _report(N))
        case("a reference disagreement also in the control -> not charged", 0,
             run("S6", vt, _trace(N), _report(N)), control=Bv)
        # 6b. a verdict flip on a DECLARED-unstable scenario is excused, but
        #     the same scenario's SUBJECT moving is not.
        case("a flip on a declared-unstable scenario -> excused", 0,
             run("S6b", _trace(N), _trace(N), _report(N, {"b-two": "diverge"})), known={"b-two"})
        rt2 = _trace(N)
        rt2["scenarios"][1]["obs"][0]["fields"]["state"] = "minecraft:stone"
        case("the subject moving on a declared-unstable scenario -> RED", 1,
             run("S6c", _trace(N), rt2, _report(N)), known={"b-two"}, name="b-two")
        # 6c. a flip present only in a SECOND control is the floor too — and
        #     the same flip with only the first control is still charged.
        case("a flip seen only by a second control -> not charged", 0,
             run("S6d", _trace(N), _trace(N), _report(N, {"c-three": "diverge"})),
             control=[B, run("Bc", _trace(N), _trace(N), _report(N, {"c-three": "diverge"}))])
        case("the same flip with only the first control -> RED", 1,
             run("S6e", _trace(N), _trace(N), _report(N, {"c-three": "diverge"})),
             name="c-three")
        # 7. a shard dropped a scenario.
        case("a scenario missing from the sharded recording -> REFUSED", 2,
             run("S7", _trace(N[:3]), _trace(N[:3]), _report(N[:3])))
        # 8. shards merged out of order.
        M = [N[2], N[3], N[0], N[1]]
        case("shards merged out of corpus order -> REFUSED", 2,
             run("S8", _trace(M), _trace(M), _report(M)))
    finally:
        shutil.rmtree(root, ignore_errors=True)
    print(f"corpus-shard-equivalence selftest: {ok} passed, {bad} failed")
    return 0 if bad == 0 else 1


# --- --check: the committed evidence against the tree ---------------------

def check():
    """The shard count ci/t2-gameplay.sh defaults to must be one the ledger
    proved, on a corpus the ledger names, and the proof must be RED-free."""
    import re
    import tomllib
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = open(os.path.join(here, "ci", "t2-gameplay.sh")).read()
    m = re.search(r'^SHARDS="\$\{RUSTMC_CORPUS_SHARDS:-(\d+)\}"$', src, re.M)
    if not m:
        print("SHARD-EQUIVALENCE --check: cannot find the SHARDS default in ci/t2-gameplay.sh",
              file=sys.stderr)
        return 1
    default = int(m.group(1))
    if default == 1:
        print("shard-equivalence: default is 1 (serial); nothing to prove")
        return 0
    try:
        led = tomllib.load(open(LEDGER, "rb"))
    except OSError:
        print(f"SHARD-EQUIVALENCE --check: default is {default} shards and there is no "
              f"ledger at {LEDGER}", file=sys.stderr)
        return 1
    runs = [r for r in led.get("run", []) if r.get("shards") == default]
    good = [r for r in runs if r.get("verdict") == "EQUIVALENT" and r.get("controlled")
            and r.get("scenarios", 0) >= 50]
    if not good:
        print(f"SHARD-EQUIVALENCE --check: ci/t2-gameplay.sh defaults to {default} shards "
              f"and {LEDGER} holds no controlled EQUIVALENT run at that count over >= 50 "
              "scenarios", file=sys.stderr)
        return 1
    print(f"shard-equivalence: default {default} shards proven by "
          + ", ".join(f"{r['id']} ({r['scenarios']} scenarios)" for r in good))
    return 0


def main(argv):
    def flag(name):
        return argv[argv.index(name) + 1] if name in argv and argv.index(name) + 1 < len(argv) else None
    if "--selftest" in argv:
        return selftest(flag("--bin"))
    if "--check" in argv:
        return check()
    if argv and argv[0] == "compare":
        need = {k: flag(k) for k in ("--bin", "--checks", "--serial", "--sharded")}
        miss = [k for k, v in need.items() if not v]
        if miss:
            refuse(f"compare needs {' '.join(miss)}")
        ctl = [argv[i + 1] for i, a in enumerate(argv) if a == "--control" and i + 1 < len(argv)]
        return compare(need["--bin"], need["--checks"], need["--serial"], ctl,
                       need["--sharded"], flag("--json"), known=known_unstable())
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
