#!/usr/bin/env python3
"""Census the vanilla surface FROM THE JAR SIDE, and ask how much of it this
repo has ever named.

THE QUESTION THIS ANSWERS, stated before the numbers, because every
completeness figure this project has produced answered a different question
than the one asked of it:

    For each vanilla registry, how many of its ids appear as a literal
    anywhere in this repo's Rust source, and how many are exercised by the
    gameplay corpus?

THE QUESTION IT DOES NOT ANSWER: how much of vanilla is implemented. Naming an
id is necessary for implementing it and nowhere near sufficient. `world_send`
names every block id it can serialise a state for, and serialising a state is
not behaviour. Every `named` column here is an UPPER BOUND on implementation
and must be read as one. The useful direction is the other one: an id named
nowhere in 157k lines of Rust is not implemented, and that column is a lower
bound on what is missing.

WHY THIS IS NOT `ci/unported-inventory.py`. That script scans our own tree for
refusal markers, so it can only find debt in files somebody already opened; its
gap counts correlate with lines written at r = 0.760 (permutation p = 0.0001).
It ranks by where work has been done. This script enumerates the denominator
from the vanilla jar's own generated reports, which cannot know where our lines
are, so the ranking is by what vanilla has and we do not. `SpawnEggItem` is the
worked example: a whole entity gap with no refusal marker anywhere, invisible
to the inventory and visible here.

Sources, all outside our control:
  oracle-work/vanilla-26.2/generated/reports/registries.json   (ids per registry)
  oracle-work/vanilla-26.2/generated/reports/blocks.json       (states per block)
  oracle-work/vanilla-26.2/generated/reports/commands.json     (command roots)
  oracle-work/vanilla-26.2/generated-data/data/minecraft/**    (datapack surface)

Regenerate them with the vanilla data generator; `ci/vanilla-server.sh` fetches
the pinned jar. Nothing here reads a number written by this project.

Usage:  ci/surface-census.py [--repo DIR] [--oracle DIR] [--json]
"""

import argparse
import json
import os
import re
import sys

DEFAULT_ORACLE = "oracle-work/vanilla-26.2"

# Registries worth a row. The comment on each says what a "named" hit means
# there, because it is not the same relationship in every row.
REGISTRIES = [
    ("minecraft:block", "blocks"),
    ("minecraft:item", "items"),
    ("minecraft:entity_type", "entity types"),
    ("minecraft:block_entity_type", "block entities"),
    ("minecraft:fluid", "fluids"),
    ("minecraft:mob_effect", "status effects"),
    ("minecraft:enchantment", "enchantments"),
    ("minecraft:menu", "menu types"),
    ("minecraft:particle_type", "particles"),
    ("minecraft:sound_event", "sound events"),
    ("minecraft:villager_profession", "villager professions"),
    ("minecraft:attribute", "attributes"),
    ("minecraft:data_component_type", "data components"),
    ("minecraft:potion", "potions"),
]

SRC_DIRS = ["crates", "plugins", "tools"]
CORPUS_GLOB = os.path.join("ci", "checks", "gameplay")

# Two shapes of id. Sound events are dotted (`block.grass.step`) and everything
# else is a bare path (`oak_door`). Harvesting only the bare shape reported
# `sound events: 0 named` on a tree that names 34 of them, which is the same
# class of defect this whole file exists to avoid: an instrument that answers a
# different question than the column heading claims.
ID_RE = re.compile(rb"[a-z][a-z0-9_]{2,}(?:\.[a-z][a-z0-9_]*)*")

# The command tree is declared in one place. Token-matching command roots
# against Rust source is worthless: `list`, `give`, `time`, `data`, `help`,
# `stop`, `team` and `seed` are all ordinary English or ordinary Rust, and the
# generous match scored 55 of 92 on a server that dispatches two.
CMD_ROOT_FILE = os.path.join("crates", "rustmc-server", "src", "commands.rs")
CMD_ROOT_RE = re.compile(r"roots:\s*vec!\[(.*?)\n\s*\],", re.S)
CMD_LEAF_RE = re.compile(r'Node::(?:leaf|branch)\("([a-z_]+)"\)')


def read_reports(oracle):
    reports = os.path.join(oracle, "generated", "reports")
    with open(os.path.join(reports, "registries.json"), "rb") as fh:
        registries = json.load(fh)
    with open(os.path.join(reports, "blocks.json"), "rb") as fh:
        blocks = json.load(fh)
    commands_path = os.path.join(reports, "commands.json")
    commands = None
    if os.path.exists(commands_path):
        with open(commands_path, "rb") as fh:
            commands = json.load(fh)
    return registries, blocks, commands


def harvest_tokens(root, dirs, suffixes):
    """Every lowercase identifier-shaped token in the tree, as bytes.

    Deliberately crude: an id counts as named if the bare path appears at all,
    quoted or not, in a comment or in code. That is the generous reading, and
    the generous reading is the right one for an upper bound.
    """
    tokens = set()
    files = 0
    for d in dirs:
        base = os.path.join(root, d)
        if not os.path.isdir(base):
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = [x for x in dirnames if x not in ("target", ".git", "golden")]
            for name in filenames:
                if not name.endswith(suffixes):
                    continue
                files += 1
                with open(os.path.join(dirpath, name), "rb") as fh:
                    tokens.update(ID_RE.findall(fh.read()))
    return tokens, files


def named(entries, tokens):
    """Ids whose bare path (after `minecraft:`) appears in the token set."""
    hit, miss = [], []
    for full in entries:
        path = full.split(":", 1)[1] if ":" in full else full
        (hit if path.encode() in tokens else miss).append(full)
    return hit, miss


def selftest(oracle):
    """Plant a tree whose answer is known, and check the census finds it.

    A census that is merely present is worth nothing: the two rows this file
    got wrong on its first run both LOOKED like measurements. So the instrument
    is shown moving. An empty tree must score zero on every row, a tree naming
    exactly three blocks must score exactly three, and a comment-only mention
    must count, because `named` is deliberately the generous reading and a
    version that silently tightened it would report progress that did not
    happen.
    """
    import shutil
    import tempfile

    oracle = oracle or os.path.join(os.path.abspath("."), DEFAULT_ORACLE)
    if not os.path.isdir(oracle):
        sys.exit("selftest needs the vanilla reports: %s" % oracle)
    registries, _, _ = read_reports(oracle)
    blocks = registries["minecraft:block"]["entries"]

    root = tempfile.mkdtemp(prefix="surface-census-selftest.")
    try:
        src = os.path.join(root, "crates", "planted", "src")
        os.makedirs(src)
        empty = os.path.join(src, "lib.rs")
        with open(empty, "w", encoding="utf-8") as fh:
            fh.write("pub fn nothing() {}\n")
        tokens, _ = harvest_tokens(root, SRC_DIRS, (".rs",))
        hit, _ = named(sorted(blocks), tokens)
        if hit:
            sys.exit("SELFTEST FAILED: empty tree named %d blocks: %s"
                     % (len(hit), hit[:5]))
        print("empty tree scores 0 blocks: ok")

        with open(empty, "w", encoding="utf-8") as fh:
            fh.write('let a = "minecraft:oak_door";\n')
            fh.write("// a comment naming stone_button, which must count\n")
            fh.write("let c = Block::redstone_wire;\n")
        tokens, _ = harvest_tokens(root, SRC_DIRS, (".rs",))
        hit, _ = named(sorted(blocks), tokens)
        want = {"minecraft:oak_door", "minecraft:stone_button",
                "minecraft:redstone_wire"}
        if set(hit) != want:
            sys.exit("SELFTEST FAILED: planted %s, census found %s"
                     % (sorted(want), sorted(hit)))
        print("three planted blocks score exactly 3, comment included: ok")

        sounds = registries["minecraft:sound_event"]["entries"]
        with open(empty, "w", encoding="utf-8") as fh:
            fh.write('play("block.grass.step");\n')
        tokens, _ = harvest_tokens(root, SRC_DIRS, (".rs",))
        hit, _ = named(sorted(sounds), tokens)
        if hit != ["minecraft:block.grass.step"]:
            sys.exit("SELFTEST FAILED: a dotted sound id was not harvested; "
                     "this is the exact defect that reported 0 of 1,968 "
                     "sound events on a tree that names 34. found %s" % hit)
        print("a dotted sound id is harvested: ok")
    finally:
        shutil.rmtree(root, ignore_errors=True)
    print("SELFTEST OK")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=".")
    ap.add_argument("--oracle", default=None)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--list-missing", metavar="REGISTRY", default=None,
                    help="print the ids of REGISTRY named nowhere, then exit")
    ap.add_argument("--selftest", action="store_true",
                    help="plant a known tree and check the counts move")
    args = ap.parse_args()

    if args.selftest:
        return selftest(args.oracle)

    repo = os.path.abspath(args.repo)
    oracle = args.oracle or os.path.join(repo, DEFAULT_ORACLE)
    if not os.path.isdir(oracle):
        sys.exit("no vanilla reports at %s; ci/vanilla-server.sh fetch, then "
                 "run the data generator" % oracle)

    registries, blocks, commands = read_reports(oracle)
    src_tokens, src_files = harvest_tokens(repo, SRC_DIRS, (".rs",))
    corpus_tokens, corpus_files = harvest_tokens(
        repo, [CORPUS_GLOB], (".toml",))

    rows = []
    for key, label in REGISTRIES:
        if key not in registries:
            continue
        entries = sorted(registries[key]["entries"].keys())
        hit, miss = named(entries, src_tokens)
        chit, _ = named(entries, corpus_tokens)
        rows.append({
            "registry": key,
            "label": label,
            "vanilla": len(entries),
            "named_in_source": len(hit),
            "named_nowhere": len(miss),
            "exercised_by_corpus": len(chit),
            "missing_sample": miss[:12],
        })

    # Blockstates: the denominator that actually governs update_shape and
    # placement work, and it is 27x the block count.
    states = sum(len(v["states"]) for v in blocks.values())

    # Command roots. `commands.json` is the real dispatcher tree, so its top
    # level is the honest denominator for "how many commands vanilla has".
    cmd_roots = sorted(commands.get("children", {}).keys()) if commands else []
    cmd_ours = []
    cmd_path = os.path.join(repo, CMD_ROOT_FILE)
    if os.path.exists(cmd_path):
        with open(cmd_path, encoding="utf-8") as fh:
            body = fh.read()
        block = CMD_ROOT_RE.search(body)
        if block:
            # Strip comments first: the module names unported commands in
            # prose, and prose is not a dispatcher.
            live = "\n".join(l for l in block.group(1).splitlines()
                             if not l.lstrip().startswith("//"))
            cmd_ours = sorted(set(CMD_LEAF_RE.findall(live)))
    cmd_named = [c for c in cmd_ours if c in cmd_roots]

    # Datapack surface: counted as files, because that is the unit a port
    # closes one of.
    gen = os.path.join(oracle, "generated-data", "data", "minecraft")
    datapack = {}
    for sub in ("worldgen/structure_set", "worldgen/structure",
                "worldgen/template_pool", "worldgen/biome", "loot_table",
                "recipe", "advancement"):
        path = os.path.join(gen, *sub.split("/"))
        n = 0
        for dirpath, _, filenames in os.walk(path):
            n += sum(1 for f in filenames if f.endswith(".json"))
        datapack[sub] = n

    out = {
        "source_files_scanned": src_files,
        "corpus_files_scanned": corpus_files,
        "blockstates_vanilla": states,
        "command_roots_vanilla": len(cmd_roots),
        "command_roots_dispatched": len(cmd_named),
        "command_roots_ours": cmd_ours,
        "datapack_files": datapack,
        "registries": rows,
    }

    if args.list_missing:
        for row in rows:
            if row["registry"] == args.list_missing or row["label"] == args.list_missing:
                entries = sorted(registries[row["registry"]]["entries"].keys())
                _, miss = named(entries, src_tokens)
                print("\n".join(miss))
                return 0
        sys.exit("no such registry: %s" % args.list_missing)

    if args.json:
        json.dump(out, sys.stdout, indent=2, sort_keys=True)
        print()
        return 0

    print("SURFACE CENSUS: vanilla 26.2 reports vs %d .rs files" % src_files)
    print("`named` is an UPPER BOUND on implementation. `nowhere` is a lower")
    print("bound on what is missing. Neither is a completeness percentage.")
    print()
    print("%-22s %8s %8s %8s %9s" %
          ("registry", "vanilla", "named", "nowhere", "in corpus"))
    for row in rows:
        print("%-22s %8d %8d %8d %9d" %
              (row["label"], row["vanilla"], row["named_in_source"],
               row["named_nowhere"], row["exercised_by_corpus"]))
    print()
    print("%-22s %8d" % ("blockstates", states))
    print("%-22s %8d %8d %8d" %
          ("command roots", len(cmd_roots), len(cmd_named),
           len(cmd_roots) - len(cmd_named)))
    print("  dispatched by us: %s" % (", ".join(cmd_named) or "none"))
    print()
    print("datapack files vanilla ships (the unit a port closes one of):")
    for k, v in datapack.items():
        print("  %-28s %6d" % (k, v))
    return 0


if __name__ == "__main__":
    sys.exit(main())
