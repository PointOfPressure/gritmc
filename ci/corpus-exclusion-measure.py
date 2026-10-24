#!/usr/bin/env python3
"""Read a corpus-exclusion experiment's run logs and say what the arms measured.

    ./ci/corpus-exclusion-measure.py <outdir>

WHAT IT READS. `ci/corpus-exclusion-experiment.sh` writes, per run, a log whose
every line is prefixed with a unix timestamp, and a `runs.tsv` row. This reads
both and derives three things the raw logs do not state:

  * the **reference recording window** of each run, from the harness's own
    "vanilla up on :PORT" and "== rustmc on :PORT" lines. Those bracket the
    phase the lock's founding claim is about: the vanilla boot has finished and
    the recorder is driving scenarios at it;
  * the **overlap** of the two runs' recording windows in a pair. This is the
    whole reason the file exists. Launching two runs together does not make
    their recordings concurrent, and a pair whose windows do not intersect
    measured nothing about concurrency. It is reported as DISCARDED and left
    out of the arm, rather than counted as a fast concurrent pair — which is
    exactly how this measurement would otherwise produce a green that cannot go
    red;
  * seconds per scenario, which is the unit the founding claim is quoted in
    ("about one scenario per ten minutes", against a solo "about 9 s").

WHAT IT REFUSES. An arm with fewer than two usable pairs, and a run whose
window markers are absent or out of order. A rate derived from a log this
parser did not actually understand is worse than no rate, because somebody
would quote it.

IT DOES NOT COMPARE RECORDINGS. Integrity is `ci/corpus-floor.sh`'s question
and is answered with the `vanilla.json` files this rig keeps beside the logs.
Rate and integrity are deliberately two instruments: the 2026-08-06 incident
was a rate collapse AND garbage output, and a design that could only see one of
them would have to guess about the other.
"""
import collections, os, re, sys, statistics

TS = re.compile(r'^(\d+\.\d+)\s(.*)$')
UP = re.compile(r'vanilla up on :(\d+)')
NEXT = re.compile(r'==\s*rustmc on :(\d+)')
NCHK = re.compile(r'subset:\s*(\d+)\s*check')


def parse_log(path):
    """-> dict(start,end,ref_start,ref_end,checks) or None."""
    first = last = ref_s = ref_e = None
    checks = None
    try:
        with open(path, errors='replace') as fh:
            for line in fh:
                m = TS.match(line)
                if not m:
                    continue
                t, body = float(m.group(1)), m.group(2)
                first = t if first is None else first
                last = t
                if ref_s is None and UP.search(body):
                    ref_s = t
                if ref_e is None and NEXT.search(body):
                    ref_e = t
                mc = NCHK.search(body)
                if mc and checks is None:
                    checks = int(mc.group(1))
    except FileNotFoundError:
        return None
    if first is None:
        return None
    return dict(start=first, end=last, ref_start=ref_s, ref_end=ref_e, checks=checks)


def load_map(out):
    """tag -> mean of the run's start and end one-minute load, from runs.tsv.

    The arms alternate, so treatment is not confounded with the time of day at
    the level of the whole experiment. That is not the same as the two arms
    having seen the same loads, and on a box that reads 4 and 110 within an
    hour it is worth checking rather than assuming. A result that only holds in
    one load band is not a result.
    """
    f = os.path.join(out, 'runs.tsv')
    m = {}
    if not os.path.exists(f):
        return m
    for line in open(f):
        c = line.rstrip('\n').split('\t')
        if len(c) < 7:
            continue
        try:
            m[c[0]] = (float(c[5]) + float(c[6])) / 2
        except ValueError:
            continue
    return m


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    out = sys.argv[1]
    pairs_f = os.path.join(out, 'pairs.tsv')
    if not os.path.exists(pairs_f):
        print(f'no pairs.tsv in {out}', file=sys.stderr)
        return 2

    runs = {}
    for name in sorted(os.listdir(out)):
        if not name.endswith('.log'):
            continue
        tag = name[:-4]
        d = parse_log(os.path.join(out, name))
        if d:
            runs[tag] = d

    loads = load_map(out)
    # Arm names are open: `serial`, and `conc2`/`conc3`/... for each level of
    # concurrency the rig was given trees for. Both founding incidents were
    # THREE concurrent runs, so a reader that only understood two would be
    # unable to report the case that actually happened.
    # DUPLICATE PAIR TAGS ARE A REFUSAL, NOT A ROW.
    #
    # Each group writes one row keyed by its tag and one log per slot named
    # `<tag>-<slot>.log`. If two groups ever share a tag they share their log
    # filenames too, so the second silently overwrites the first and the
    # ensemble quietly shrinks while still looking complete. That is exactly
    # what a loop-variable leak in the rig produced on 2026-08-18: `group`
    # assigned the caller's rep counter, rep 1's concurrent arms were tagged as
    # rep 2's, and rep 2 then overwrote them. Nothing failed; there were simply
    # fewer runs than reps. Refuse instead.
    seen_tags = {}
    dupes = []
    for line in open(pairs_f):
        f = line.rstrip('\n').split('\t')
        if len(f) < 2:
            continue
        if f[0] in seen_tags:
            dupes.append(f[0])
        seen_tags[f[0]] = True
    if dupes:
        print(f'REFUSING: duplicate pair tag(s) in pairs.tsv: {sorted(set(dupes))}',
              file=sys.stderr)
        print('  Two groups shared a tag, so they shared their log filenames and one',
              file=sys.stderr)
        print('  overwrote the other. The ensemble is smaller than it looks.',
              file=sys.stderr)
        return 2

    arms = collections.defaultdict(list)
    # Every arm NAMED in pairs.tsv, whether or not any of its runs survived the
    # discard rules. Iterating the results dict instead would let an arm whose
    # runs were all discarded vanish from the report and from the exit status —
    # the report would then be silent about the arm that failed hardest.
    arms_seen = []
    discarded = []
    rows = []
    for line in open(pairs_f):
        f = line.rstrip('\n').split('\t')
        if len(f) < 4:
            continue
        pt, arm, ra, rb = f[0], f[1], f[2], f[3]
        if arm not in arms_seen:
            arms_seen.append(arm)
        a, b = runs.get(f'{pt}-a'), runs.get(f'{pt}-b')
        if not a or not b:
            discarded.append((pt, arm, 'a run produced no parsable log'))
            continue
        if ra != '0' or rb != '0':
            discarded.append((pt, arm, f'non-zero exit (a={ra} b={rb})'))
            continue
        if not all(x['ref_start'] and x['ref_end'] for x in (a, b)):
            discarded.append((pt, arm, 'recording window markers missing'))
            continue
        ov = min(a['ref_end'], b['ref_end']) - max(a['ref_start'], b['ref_start'])
        if arm.startswith('conc') and ov <= 0:
            discarded.append((pt, arm, f'recordings did not overlap ({ov:.1f}s)'))
            continue
        for who, d in (('a', a), ('b', b)):
            dur = d['ref_end'] - d['ref_start']
            n = d['checks'] or 0
            sps = dur / n if n else None
            if sps is None:
                discarded.append((pt, arm, 'no scenario count'))
                break
            arms[arm].append(sps)
            rows.append((pt, arm, who, dur, n, sps, ov, loads.get(f'{pt}-{who}')))

    print(f'== runs, per reference-recording phase   ({out})')
    print(f'{"pair":8s} {"arm":7s} {"tree":4s} {"ref_s":>8s} {"n":>3s} {"s/scen":>8s} '
          f'{"overlap_s":>10s} {"load":>7s}')
    for pt, arm, who, dur, n, sps, ov, ld in rows:
        ls = f'{ld:7.1f}' if ld is not None else '      -'
        print(f'{pt:8s} {arm:7s} {who:4s} {dur:8.1f} {n:3d} {sps:8.2f} {ov:10.1f} {ls}')

    print()
    if discarded:
        print('== discarded')
        for pt, arm, why in discarded:
            print(f'  {pt:8s} {arm:7s} {why}')
        print()

    print('== arms')
    stats = {}
    rc = 0
    for arm in sorted(arms_seen):
        v = arms[arm]
        if len(v) < 2:
            print(f'  {arm:7s} N={len(v)} — REFUSING to summarise fewer than two runs')
            rc = 1
            continue
        stats[arm] = statistics.median(v)
        print(f'  {arm:7s} N={len(v):2d}  median {statistics.median(v):6.2f} s/scen  '
              f'mean {statistics.mean(v):6.2f}  min {min(v):6.2f}  max {max(v):6.2f}')

    # Stratify by load. The box reads 4 and 110 within the hour, and
    # `readiness-falsify` was RED at 11.8 and GREEN at 3.5 on an identical
    # tree, so a ratio quoted without saying which loads produced it is a
    # number waiting to be withdrawn.
    withload = [r for r in rows if r[7] is not None]
    if len(withload) >= 4:
        # THE ARMS CANNOT BE LOAD-MATCHED, AND THAT FAVOURS THE LOCK.
        # Running two recordings at once *is* an increase in load, so the
        # concurrent arm sits at systematically higher load than the serial one
        # by construction and no design can separate them. The split below
        # therefore partly re-expresses the arm rather than controlling for it.
        # It is reported anyway because the direction is the useful part: the
        # concurrent arm is measured under the worse conditions of the two, so
        # a concurrent arm that matches serial has matched it from behind.
        for arm in sorted(arms_seen):
            v = [r[7] for r in withload if r[1] == arm]
            if v:
                print(f'  arm {arm:7s} ran at load median {statistics.median(v):5.1f} '
                      f'(min {min(v):.1f}, max {max(v):.1f}, N={len(v)})')
        med = statistics.median(r[7] for r in withload)
        print()
        print(f'== by load, split at the median one-minute load ({med:.1f})')
        for band, keep in (('low ', lambda l: l <= med), ('high', lambda l: l > med)):
            line = f'  {band}'
            for arm in sorted(arms_seen):
                v = [r[5] for r in withload if r[1] == arm and keep(r[7])]
                if len(v) >= 2:
                    line += f'  {arm} N={len(v):2d} median {statistics.median(v):6.2f}'
                else:
                    line += f'  {arm} N={len(v):2d} (too few)      '
            print(line)
        for band, keep in (('low ', lambda l: l <= med), ('high', lambda l: l > med)):
            sv = [r[5] for r in withload if r[1] == 'serial' and keep(r[7])]
            if len(sv) < 2 or statistics.median(sv) <= 0:
                continue
            for arm in sorted(a for a in arms_seen if a.startswith('conc')):
                cv = [r[5] for r in withload if r[1] == arm and keep(r[7])]
                if len(cv) >= 2:
                    print(f'  {band} {arm}/serial = '
                          f'{statistics.median(cv) / statistics.median(sv):.2f}x')

    if 'serial' in stats and stats['serial'] > 0:
        print()
        for arm in sorted(a for a in stats if a.startswith('conc')):
            r = stats[arm] / stats['serial']
            print(f'== {arm} / serial = {r:.2f}x  (median seconds per scenario)')

        # THE BOUND, NOT THE POINT ESTIMATE. A ratio near 1 is what an
        # underpowered experiment produces when it measures nothing, so the
        # number worth quoting is the upper end of the interval: the largest
        # slowdown these data still permit. Normal-approximation CI on the
        # difference of means, which is ample here because the arms are tight
        # (sd ~0.15 s against a mean of ~12 s) and the effect being excluded is
        # two orders of magnitude away.
        sv = [r[5] for r in rows if r[1] == 'serial']
        for arm in sorted(a for a in arms_seen if a.startswith('conc')):
            cv = [r[5] for r in rows if r[1] == arm]
            if len(sv) < 2 or len(cv) < 2:
                continue
            ms, mc = statistics.mean(sv), statistics.mean(cv)
            se = (statistics.stdev(sv) ** 2 / len(sv)
                  + statistics.stdev(cv) ** 2 / len(cv)) ** 0.5
            hi = (mc - ms + 1.96 * se) / ms
            lo = (mc - ms - 1.96 * se) / ms
            print(f'   {arm}: mean {mc:.3f} vs serial {ms:.3f} s/scen, '
                  f'95% CI on the ratio {1 + lo:.3f}x .. {1 + hi:.3f}x')
            print(f'   -> EXCLUDES any slowdown greater than {100 * hi:.1f}% '
                  f'(N={len(cv)} vs {len(sv)})')
        print('   the founding claim is a collapse of roughly two orders of magnitude;')
        print('   a ratio near 1 is the null, and is not on its own a case for changing the lock.')
    return rc


if __name__ == '__main__':
    sys.exit(main())
