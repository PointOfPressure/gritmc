#!/usr/bin/env bash
# dump-structure-tables.sh — the four structure tables, out of the pinned jar.
#
#     ci/dump-structure-tables.sh
#
# Writes, all four or none:
#
#     data/vanilla/structure_sets.json        20 structure sets
#     data/vanilla/structure_defs.json        34 structure definitions
#     data/vanilla/template_pools.json        188 jigsaw template pools
#     data/vanilla/structure_templates.json   1,212 .nbt templates, as a manifest
#
# # Why this script exists
#
# `data/vanilla/structure_sets.json` was already on every tree on the box and
# **no script in this repo produced it.** Nine source files read it; nothing
# could regenerate it; it carried no `.provenance` stamp, so
# `ci/t2-gameplay.sh`'s foreign-table refusal could not see it and
# `ci/gen-gameplay-tables.sh`'s derived list did not require it. A table in
# that state is the 2026-08-03 shared-`data/vanilla` failure waiting to
# happen: whatever tree last wrote it decides what every other tree measures,
# and nobody can tell.
#
# This reproduces that file **byte for byte** — checked, not asserted; see the
# `--check-existing` mode below — and adds the three tables beside it that the
# structure census needs and nothing had.
#
# # Where the bytes come from
#
# The pinned `server-26.2.jar`. Structure sets, structure definitions and
# template pools are datapack JSON and ship in the jar verbatim, so this is an
# `unzip` and a merge rather than a transcription — the clean-room rule is not
# in play, this is vanilla's own data.
#
# # The cross-check, and why it is not decoration
#
# `ci/vanilla-server.sh data` runs Mojang's own datagen, which writes the same
# families back out **through the server's codecs**. A codec applies defaults
# and normalises shapes, so a field that is optional in the shipped JSON and
# defaulted at load would differ between the two sources — and the generator
# reads the codec's view, not the file's. Measured 2026-09-05: all 20 sets and
# all 34 structures are identical between the jar and the datagen, so the raw
# file *is* the codec's view for these two families and reading the jar loses
# nothing.
#
# The check runs whenever the datagen extract is on disk and is a **refusal**
# on any disagreement. When it is absent the script says so on stderr and
# carries on, because running a JVM datagen is minutes and this table is
# needed by every tree's setup — but it never passes silently.
#
# # Determinism
#
# Everything is emitted with `sort_keys=True` and a fixed indent, and the
# script builds each table twice and refuses if the two runs differ. A table
# that is not reproducible poisons every measurement taken after it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
JAR="$ROOT/oracle-work/vanilla-$VER/versions/$VER/server-$VER.jar"
GEN="$ROOT/oracle-work/vanilla-$VER/generated-data/data/minecraft/worldgen"
OUT="$ROOT/data/vanilla"

# REFUSAL, NOT A SKIP — `ci/dump-flat-preset.sh` has the paragraph on why.
# This script is not asked to run, it is asked to produce four tables, and
# there is no state of the world in which "the jar was missing" is a success:
# the caller would find another tree's copy of `structure_sets.json` on disk,
# stamp it as this tree's, and every structure measurement after that would be
# somebody else's.
if [ ! -s "$JAR" ]; then
    echo "dump-structure-tables: no pinned jar at $JAR" >&2
    echo "    run ci/vanilla-server.sh first; cannot generate the structure tables" >&2
    exit 1
fi

mkdir -p "$OUT"
python3 - "$JAR" "$GEN" "$OUT" "${1:-}" <<'PY'
import binascii
import json
import os
import sys
import zipfile

jar, gen_root, out_dir, mode = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]

FAMILIES = {
    "structure_sets.json": "data/minecraft/worldgen/structure_set/",
    "structure_defs.json": "data/minecraft/worldgen/structure/",
}
POOLS = "data/minecraft/worldgen/template_pool/"
TEMPLATES = "data/minecraft/structure/"


def build(zf):
    """Every table, from one open jar. Called twice; the results must agree."""
    names = zf.namelist()
    tables = {}

    for table, prefix in FAMILIES.items():
        entries = {}
        for n in names:
            if not n.startswith(prefix) or not n.endswith(".json"):
                continue
            key = "minecraft:" + n[len(prefix):-len(".json")]
            entries[key] = json.loads(zf.read(n))
        tables[table] = entries

    # Template pools, as a MANIFEST rather than a second copy of the files.
    # `data/vanilla/template_pool/` already holds all 188 verbatim; what
    # nothing had is the shape a census needs — how many elements each pool
    # offers, what kinds they are, and which templates they name. An element
    # kind is what decides whether a pool is reachable by this port at all:
    # `single_pool_element` and `list_pool_element` place a template,
    # `feature_pool_element` places a configured feature, `empty_pool_element`
    # places nothing.
    pools = {}
    for n in names:
        if not n.startswith(POOLS) or not n.endswith(".json"):
            continue
        key = "minecraft:" + n[len(POOLS):-len(".json")]
        doc = json.loads(zf.read(n))
        kinds = {}
        templates = []

        def walk(el):
            t = el.get("element_type", "")
            kinds[t] = kinds.get(t, 0) + 1
            loc = el.get("location")
            if isinstance(loc, str):
                templates.append(loc)
            for sub in el.get("elements", []):
                walk(sub)

        for entry in doc.get("elements", []):
            walk(entry.get("element", {}))
        pools[key] = {
            "fallback": doc.get("fallback"),
            "entries": len(doc.get("elements", [])),
            "element_kinds": dict(sorted(kinds.items())),
            "templates": sorted(set(templates)),
        }
    tables["template_pools.json"] = pools

    # The .nbt templates. Size and CRC come out of the zip's own central
    # directory, so this manifest pins the bytes without unpacking 1,212 files
    # — a template that changed under a same-named path would move its CRC.
    templates = {}
    for info in zf.infolist():
        n = info.filename
        if not n.startswith(TEMPLATES) or not n.endswith(".nbt"):
            continue
        templates["minecraft:" + n[len(TEMPLATES):-len(".nbt")]] = {
            "size": info.file_size,
            "crc32": f"{info.CRC & 0xFFFFFFFF:08x}",
        }
    tables["structure_templates.json"] = templates
    return tables


with zipfile.ZipFile(jar) as zf:
    first = build(zf)
with zipfile.ZipFile(jar) as zf:
    second = build(zf)
if first != second:
    raise SystemExit("dump-structure-tables: two runs of this dumper disagree")

# --- the datagen cross-check ------------------------------------------------
checked = []
for table, prefix in FAMILIES.items():
    family = prefix.rstrip("/").rsplit("/", 1)[-1]
    d = os.path.join(gen_root, family)
    if not os.path.isdir(d):
        continue
    theirs = {}
    for f in sorted(os.listdir(d)):
        if f.endswith(".json"):
            theirs["minecraft:" + f[: -len(".json")]] = json.load(open(os.path.join(d, f)))
    ours = first[table]
    if theirs != ours:
        only_ours = sorted(set(ours) - set(theirs))
        only_theirs = sorted(set(theirs) - set(ours))
        moved = sorted(k for k in ours if k in theirs and ours[k] != theirs[k])
        raise SystemExit(
            f"dump-structure-tables: {family} disagrees with the datagen extract — "
            f"{len(only_ours)} only in the jar, {len(only_theirs)} only in the datagen, "
            f"{len(moved)} with different content: {(only_ours + only_theirs + moved)[:5]}. "
            "The codec's view is the one the generator reads; do not ship the jar's."
        )
    checked.append(f"{family}={len(ours)}")
if checked:
    print(f"  datagen cross-check: {' '.join(checked)} identical")
else:
    print(
        "  datagen cross-check: SKIPPED, no extract at "
        f"{gen_root} — run ci/vanilla-server.sh data to take it",
        file=sys.stderr,
    )

# --- byte-compatibility with the file that had no generator -----------------
existing = os.path.join(out_dir, "structure_sets.json")
if mode == "--check-existing":
    if not os.path.exists(existing):
        raise SystemExit(f"--check-existing: {existing} is not there to compare against")
    have = json.load(open(existing))
    if have != first["structure_sets.json"]:
        raise SystemExit(
            "--check-existing: this dumper does not reproduce the structure_sets.json "
            "already on disk. That file has no generator, so a difference means one of "
            "the two is not the pinned jar's data and the tree cannot tell which."
        )
    print(f"  --check-existing: reproduces {existing} exactly ({len(have)} sets)")
    raise SystemExit(0)

# --- the .nbt extract, made reproducible ------------------------------------
#
# `data/vanilla/structure/` is a version-pinned jar extract that, like
# `structure_sets.json`, no script in this repo produced. This unpacks it from
# the same jar and verifies every file already there against the zip's own
# CRC, so from here the directory is derived rather than inherited.
#
# Measured 2026-09-05 on the copy every tree on the box was seeded from: 1,195
# of the jar's 1,212 present and all 1,195 CRC-clean, 17 absent — `empty.nbt`
# and the sixteen `fossil/*.nbt`, which belong to the overworld `fossil`
# FEATURE rather than to `nether_fossils`. A CRC mismatch is a refusal: it
# means the bytes under a pinned name moved, and every structure measurement
# taken against them is void.
tdir = os.path.join(out_dir, "structure")
written = verified = 0
mismatched = []
with zipfile.ZipFile(jar) as zf:
    for info in zf.infolist():
        n = info.filename
        if not (n.startswith(TEMPLATES) and n.endswith(".nbt")):
            continue
        dest = os.path.join(tdir, n[len(TEMPLATES):])
        want = info.CRC & 0xFFFFFFFF
        if os.path.exists(dest):
            with open(dest, "rb") as fh:
                got = binascii.crc32(fh.read()) & 0xFFFFFFFF
            if got != want:
                mismatched.append(n[len(TEMPLATES):])
            else:
                verified += 1
            continue
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        with open(dest, "wb") as fh:
            fh.write(zf.read(n))
        written += 1
if mismatched:
    raise SystemExit(
        f"dump-structure-tables: {len(mismatched)} template(s) under "
        f"{tdir} do not match the pinned jar's CRC: {mismatched[:5]}"
    )
print(f"  structure/: {verified} verified against the jar CRC, {written} extracted")

for table, entries in first.items():
    path = os.path.join(out_dir, table)
    with open(path, "w") as fh:
        json.dump(entries, fh, indent=1, sort_keys=True)
        fh.write("\n")
    print(f"  {table}: {len(entries)} entries -> {path}")

sets = first["structure_sets.json"]
defs = first["structure_defs.json"]
named = sorted({s["structure"] for v in sets.values() for s in v.get("structures", [])})
print(
    f"structure tables: {len(sets)} sets naming {len(named)} structures, "
    f"{len(defs)} definitions, {len(first['template_pools.json'])} pools, "
    f"{len(first['structure_templates.json'])} templates"
)
PY
