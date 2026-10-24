#!/usr/bin/env python3
"""Falsification sweep for this branch's three changes.

Injects one bug at a time into the source, runs the tests that should catch
it, and reports EVERY injection including the ones that get through.

Two rules learned the hard way here and mechanised rather than remembered:

  * a CONTROL run before and after. An `rsync -a` restore once gave a file an
    older mtime than the injected copy, so cargo tested a stale artifact and
    the whole sweep was fabricated verdicts.
  * every write `touch`es the file, for the same reason.

usage: falsify-ruined-portal.py
"""
import hashlib
import os
import pathlib
import signal
import subprocess
import sys
import time

# A sweep takes ~15 minutes; without this the log is empty until it ends.
sys.stdout.reconfigure(line_buffering=True)

ROOT = pathlib.Path(__file__).resolve().parent.parent
RP = ROOT / "crates/rustmc-worldgen/src/structure/ruined_portal.rs"
TP = ROOT / "crates/rustmc-worldgen/src/structure/template.rs"
SS = ROOT / "crates/rustmc-worldgen/src/feature/state_spec.rs"
PL = ROOT / "crates/rustmc-worldgen/src/structure/place.rs"

# (name, file, find, replace, test-selector)
# Debug, not release: these are logic tests, and a release rebuild per
# injection (thin LTO, codegen-units=1) costs ~35s against ~8s. Sixteen
# rebuilds is the difference between a 4-minute sweep and a 15-minute one.
UNIT = "cargo test -p rustmc-worldgen --lib"
GOLD_X = "cargo test -p rustmc-worldgen --test golden_cross_collision_blocks"
GOLD_T = "cargo test -p rustmc-worldgen --test golden_structure_template"
GOLD_S = "cargo test -p rustmc-worldgen --test golden_ruined_portal_setups"

INJECTIONS = [
    ("1  sample: drop the 1.0 short circuit (always draw)", RP,
     "    if probability == 1.0 {\n        return true;\n    }\n", "", UNIT),
    ("2  sample: drop the 0.0 short circuit (always draw)", RP,
     "    if probability == 0.0 {\n        return false;\n    }\n", "", UNIT),
    ("3  mirror threshold inverted", RP,
     "let mirror = if random.next_f32() < 0.5 {",
     "let mirror = if random.next_f32() >= 0.5 {", UNIT),
    ("4  rotation array reversed", RP,
     "        Rotation::None,\n        Rotation::Clockwise90,\n        Rotation::Clockwise180,\n        Rotation::Counterclockwise90,\n    ];",
     "        Rotation::Counterclockwise90,\n        Rotation::Clockwise180,\n        Rotation::Clockwise90,\n        Rotation::None,\n    ];", UNIT),
    ("5  PORTALS: swap portal_3 and portal_4", RP,
     '    "ruined_portal/portal_3",\n    "ruined_portal/portal_4",',
     '    "ruined_portal/portal_4",\n    "ruined_portal/portal_3",', UNIT),
    ("6  giant-portal probability 0.05 -> 0.5", RP,
     "pub const PROBABILITY_OF_GIANT_PORTAL: f32 = 0.05;",
     "pub const PROBABILITY_OF_GIANT_PORTAL: f32 = 0.5;", UNIT),
    ("7  weighted pick: f <= 0.0 instead of f < 0.0", RP,
     "            if f < 0.0 {", "            if f <= 0.0 {", UNIT),
    ("8  empty interval returns min, not max", RP,
     "    if min < max {\n        random.next_i32_between_inclusive(min, max)\n    } else {\n        max\n    }",
     "    if min < max {\n        random.next_i32_between_inclusive(min, max)\n    } else {\n        min\n    }", UNIT),
    ("9  the corner scan stops at 2 opaque, not 3", RP,
     "                if opaque == 3 {", "                if opaque == 2 {", UNIT),
    ("10 cross-collision: swap CW90 and CCW90 sources", TP,
     '        (Rotation::Clockwise90, "north") => "west",',
     '        (Rotation::Clockwise90, "north") => "east",', GOLD_T),
    ("11 cross-collision: treat vine as cross-collision", TP,
     '    "minecraft:acacia_fence",', '    "minecraft:acacia_fence",\n    "minecraft:vine",', GOLD_X),
    ("12 cross-collision: drop nether_brick_fence", TP,
     '    "minecraft:nether_brick_fence",\n', "", GOLD_X),
    ("13 parse_for_block: write the string verbatim (the old bug)", SS,
     "    let mut properties = split(base);\n    for (key, value) in split(state) {",
     "    let mut properties = split(state);\n    for (key, value) in split(state) {", UNIT),
    ("14 parse_for_block: ignore the named properties", SS,
     "        properties.insert(key, value);", "        let _ = value;", UNIT),
    ("15 parse_for_block: accept an unknown property", SS,
     "        if !properties.contains_key(&key) {", "        if false {", UNIT),

    # ---- the piece: postProcess, its processors, and the mirror -------------
    # Added with the block port. Every one of these changes what the piece
    # WRITES rather than how it is planned, which is the half the start's 4/4
    # gate says nothing about.
    ("16 mirror: FRONT_BACK reflects z instead of x", TP,
     '        (Mirror::FrontBack, "east") => "west",\n        (Mirror::FrontBack, "west") => "east",\n        (Mirror::LeftRight, "north") => "south",\n        (Mirror::LeftRight, "south") => "north",',
     '        (Mirror::FrontBack, "north") => "south",\n        (Mirror::FrontBack, "south") => "north",\n        (Mirror::LeftRight, "east") => "west",\n        (Mirror::LeftRight, "west") => "east",', UNIT),
    ("17 mirror: apply the flip to `axis` as well", TP,
     '            Some(("facing", value)) => {\n                let flipped = mirror_facing(value, mirror);',
     '            Some(("axis", _)) => "axis=y".to_string(),\n            Some(("facing", value)) => {\n                let flipped = mirror_facing(value, mirror);', UNIT),
    ("18 mirror: a vine is treated as cross-collision", TP,
     "    let cross = is_cross_collision(name);\n    let props: Vec<String> = raw\n        .split(',')\n        .map(|pair| match pair.split_once('=') {\n            Some((key @ (\"north\" | \"east\" | \"south\" | \"west\"), _)) if cross => {\n                let from = mirror_facing(key, mirror);",
     "    let cross = true;\n    let props: Vec<String> = raw\n        .split(',')\n        .map(|pair| match pair.split_once('=') {\n            Some((key @ (\"north\" | \"east\" | \"south\" | \"west\"), _)) if cross => {\n                let from = mirror_facing(key, mirror);", UNIT),
    ("19 place_piece: rotate before mirror instead of after", PL,
     "        let state = rotate_state(&mirror_state(&info.state, mirror), rotation);",
     "        let state = mirror_state(&rotate_state(&info.state, rotation), mirror);", UNIT),
    ("20 place_piece: drop the mirror from `transform`", PL,
     "        let (x, y, z) = transform(block.pos, mirror, rotation, pivot);",
     "        let (x, y, z) = transform(block.pos, Mirror::None, rotation, pivot);", UNIT),
    ("21 block_age: build the candidate arrays lazily (THE registered risk)", PL,
     "        let non_mossy = [\n            default(\"minecraft:cracked_stone_bricks\"),\n            random_facing_stairs(world, random, \"minecraft:stone_brick_stairs\"),\n        ];\n        let mossy = [\n            default(\"minecraft:mossy_stone_bricks\"),\n            random_facing_stairs(world, random, \"minecraft:mossy_stone_brick_stairs\"),\n        ];\n        return Some(random_block(random, mossiness, &non_mossy, &mossy));",
     "        if random.next_f32() < mossiness {\n            let mossy = [\n                default(\"minecraft:mossy_stone_bricks\"),\n                random_facing_stairs(world, random, \"minecraft:mossy_stone_brick_stairs\"),\n            ];\n            return Some(mossy[usize::try_from(random.next_i32_bound(2)).unwrap()].clone());\n        }\n        let non_mossy = [\n            default(\"minecraft:cracked_stone_bricks\"),\n            random_facing_stairs(world, random, \"minecraft:stone_brick_stairs\"),\n        ];\n        return Some(non_mossy[usize::try_from(random.next_i32_bound(2)).unwrap()].clone());", UNIT),
    ("22 block_age: slab tests against 0.5 instead of mossiness", PL,
     "    if world.block_in_tag(\"minecraft:slabs\", block) {\n        // `maybeReplaceSlab` — one draw, and against `mossiness`, not 0.5.\n        return (random.next_f32() < mossiness)",
     "    if world.block_in_tag(\"minecraft:slabs\", block) {\n        // `maybeReplaceSlab` — one draw, and against `mossiness`, not 0.5.\n        return (random.next_f32() < 0.5)", UNIT),
    ("23 block_age: obsidian uses mossiness instead of 0.15", PL,
     "        return (random.next_f32() < 0.15).then(|| default(\"minecraft:crying_obsidian\"));",
     "        return (random.next_f32() < mossiness).then(|| default(\"minecraft:crying_obsidian\"));", UNIT),
    # THIS INJECTION WAS DEAD FOR ONE COMMIT AND THE SWEEP SAID SO. `f7254194`
    # rewrote `!(x < 0.5)` to `x >= 0.5` to clear `neg_cmp_op_on_partial_ord`,
    # and this pattern stopped matching: the 2026-08-09 sweep printed
    # `24 … NO-OP  <-- pattern not found, injection did nothing`. That is the
    # NO-OP guard working exactly as designed — a find/replace sweep whose
    # patterns rot silently is a sweep that reports 48 of 48 while testing 47.
    # Re-pointed at the surviving form; the sense is still inverted.
    ("24 block_age: the stone gate opens at < 0.5 instead of >= 0.5", PL,
     "    ) {\n        // `maybeReplaceFullStoneBlock`\n        if random.next_f32() >= 0.5 {",
     "    ) {\n        // `maybeReplaceFullStoneBlock`\n        if random.next_f32() < 0.5 {", UNIT),
    ("25 block_age: NON_MOSSY_REPLACEMENTS are bricks, not slabs", PL,
     "        let non_mossy = [\n            default(\"minecraft:stone_slab\"),\n            default(\"minecraft:stone_brick_slab\"),\n        ];",
     "        let non_mossy = [\n            default(\"minecraft:stone\"),\n            default(\"minecraft:stone_bricks\"),\n        ];", UNIT),
    ("26 block_age: netherrack falls into the stone arm", PL,
     '        "minecraft:stone_bricks" | "minecraft:stone" | "minecraft:chiseled_stone_bricks"',
     '        "minecraft:stone_bricks" | "minecraft:stone" | "minecraft:netherrack"', UNIT),
    ("27 with_properties_of: add properties the target lacks", PL,
     "    if !seen {\n        return state.to_string();\n    }",
     "    if !seen {\n        return format!(\"{state}\");\n    }", UNIT),
    ("28 make_settings: air_pocket picks the wrong ignore processor", RP,
     "    let ignore = if properties.air_pocket {\n        crate::structure::place::structure_block()\n    } else {\n        crate::structure::place::structure_and_air()\n    };",
     "    let ignore = if properties.air_pocket {\n        crate::structure::place::structure_and_air()\n    } else {\n        crate::structure::place::structure_block()\n    };", UNIT),
    ("29 make_settings: the magma rule is added even when cold", RP,
     "    if !properties.cold {\n        rules.push(replace_rule_random(\n            \"minecraft:netherrack\",",
     "    if true {\n        rules.push(replace_rule_random(\n            \"minecraft:netherrack\",", UNIT),
    ("30 make_settings: BlockAge runs before the RuleProcessor", RP,
     "        ignore,\n        Processor::Rule(rules),\n        Processor::BlockAge(properties.mossiness),",
     "        ignore,\n        Processor::BlockAge(properties.mossiness),\n        Processor::Rule(rules),", UNIT),
    ("31 make_settings: a JigsawReplacement is added to the chain", RP,
     "    let mut out = vec![\n        ignore,\n        Processor::Rule(rules),",
     "    let mut out = vec![\n        ignore,\n        Processor::JigsawReplacement,\n        Processor::Rule(rules),", UNIT),
    ("32 lava rule: the cold and ocean-floor branches are swapped", RP,
     "    if placement == VerticalPlacement::OnOceanFloor {\n        return replace_rule(\"minecraft:lava\", \"minecraft:magma_block\");\n    }\n    if properties.cold {\n        return replace_rule(\"minecraft:lava\", \"minecraft:netherrack\");\n    }",
     "    if properties.cold {\n        return replace_rule(\"minecraft:lava\", \"minecraft:netherrack\");\n    }\n    if placement == VerticalPlacement::OnOceanFloor {\n        return replace_rule(\"minecraft:lava\", \"minecraft:magma_block\");\n    }", UNIT),
    ("33 lava rule: the warm branch spends no draw (BlockMatch)", RP,
     "    replace_rule_random(\n        \"minecraft:lava\",\n        PROBABILITY_OF_MAGMA_INSTEAD_OF_LAVA,\n        \"minecraft:magma_block\",\n    )",
     "    replace_rule(\"minecraft:lava\", \"minecraft:magma_block\")", UNIT),
    ("34 placeNetherrackOrMagma: drop the !cold short circuit", RP,
     "    if !properties.cold && random.next_f32() < PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK {",
     "    if random.next_f32() < PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK {", UNIT),
    ("35 drip column: draw before testing the counter", RP,
     "    while i > 0 && random.next_f32() < 0.5 {",
     "    while random.next_f32() < 0.5 && i > 0 {", UNIT),
    ("36 drip column: nine iterations instead of eight", RP,
     "    let mut i = 8;", "    let mut i = 9;", UNIT),
    ("37 canBeReplaced: obsidian is replaceable", RP,
     '    if block == "minecraft:air" || block == "minecraft:obsidian" {',
     '    if block == "minecraft:air" {', UNIT),
    ("38 canBeReplaced: lava is refused in the nether too", RP,
     "    if placement != VerticalPlacement::InNether && block == \"minecraft:lava\" {",
     "    if block == \"minecraft:lava\" {", UNIT),
    ("39 spreadNetherrack: the weight ramp is reversed", RP,
     "        1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,",
     "        0.2, 0.4, 0.6, 0.7, 0.8, 0.9, 0.9, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,", UNIT),
    ("40 spreadNetherrack: nextFloat instead of nextDouble", RP,
     "            if random.next_f64() >= f64::from(weight) {",
     "            if random.next_f32() >= weight {", UNIT),
    ("41 spreadNetherrack: the y band is 4 instead of 3", RP,
     "            if (y - b.min_y).abs() > 3 {", "            if (y - b.min_y).abs() > 4 {", UNIT),
    ("42 spreadNetherrack: a surface portal uses min(), a buried one the surface", RP,
     "            let y = if on_surface {\n                surface\n            } else {\n                b.min_y.min(surface)\n            };",
     "            let y = if on_surface {\n                b.min_y.min(surface)\n            } else {\n                surface\n            };", UNIT),
    ("43 box_center: uses the max corner, not the span", RP,
     "        b.min_x + (b.max_x - b.min_x + 1) / 2,\n        b.min_y + (b.max_y - b.min_y + 1) / 2,\n        b.min_z + (b.max_z - b.min_z + 1) / 2,",
     "        (b.min_x + b.max_x) / 2,\n        (b.min_y + b.max_y) / 2,\n        (b.min_z + b.max_z) / 2,", UNIT),
    ("44 postProcess: the clip is the chunk, not the encapsulated box", RP,
     "    let clip = box_encapsulate(writable_area, template_box);",
     "    let clip = writable_area;", UNIT),
    ("45 postProcess: the centre guard is dropped (runs in every chunk)", RP,
     "    if !box_contains(writable_area, box_center(template_box)) {\n        return false;\n    }",
     "", UNIT),
    ("46 maybeAddLeavesAbove: the draw becomes conditional", RP,
     "    if random.next_f32() >= 0.5 {\n        return;\n    }\n    let state = world.block_at(pos.0, pos.1, pos.2);\n    if state.split('[').next().unwrap_or(&state) != \"minecraft:netherrack\" {\n        return;\n    }",
     "    let state = world.block_at(pos.0, pos.1, pos.2);\n    if state.split('[').next().unwrap_or(&state) != \"minecraft:netherrack\" {\n        return;\n    }\n    if random.next_f32() >= 0.5 {\n        return;\n    }", UNIT),
    ("47 drip columns: the bounds include the box rim", RP,
     "    for x in (b.min_x + 1)..b.max_x {\n        for z in (b.min_z + 1)..b.max_z {",
     "    for x in b.min_x..=b.max_x {\n        for z in b.min_z..=b.max_z {", UNIT),
    ("49 place_piece: the LootTableSeed draw is not taken", PL,
     "        if info.has_nbt && is_randomizable_container(block_of(&state)) {\n            if let Some(random) = random.as_deref_mut() {\n                let _ = random.next_i64();\n            }\n        }\n", "", UNIT),
    ("50 place_piece: nbt OR container, instead of both", PL,
     "        if info.has_nbt && is_randomizable_container(block_of(&state)) {",
     "        if info.has_nbt || is_randomizable_container(block_of(&state)) {", UNIT),
    ("51 the jigsaw pass writes the jigsaw back instead of its final_state", PL,
     "        let resolved = parse_final_state(world, &jigsaw.final_state);",
     "        let resolved = palette[block.state].clone();", UNIT),
    ("48 HORIZONTAL order is the Direction ordinal, not the plane's", PL,
     'const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];',
     'const HORIZONTAL: [&str; 4] = ["north", "south", "west", "east"];', UNIT),
    # 52 and 53 are the defect this branch fixed and its near miss.
    # `getRandomFacingStairs` sets FACING **and** HALF, so it is two draws and
    # not one, and it is called twice per qualifying stone block. Until
    # 2026-08-09 the port drew only FACING: 14 blocks wrong in the r=16
    # overworld portal box, all at y 43..44.
    ("52 getRandomFacingStairs draws FACING but not HALF", PL,
     '    let facing = HORIZONTAL[usize::try_from(random.next_i32_bound(4)).expect("0..4 is a usize")];\n'
     '    let half = HALF[usize::try_from(random.next_i32_bound(2)).expect("0..2 is a usize")];\n'
     '    set_prop(&set_prop(&base, "facing", facing), "half", half)',
     '    let facing = HORIZONTAL[usize::try_from(random.next_i32_bound(4)).expect("0..4 is a usize")];\n'
     '    set_prop(&base, "facing", facing)', UNIT),
    # The order-only sibling: this one spends exactly the right draws and
    # lands every aged stair the other way up, so no draw-count test and no
    # stream-offset test can see it. `Half.values()` is [TOP, BOTTOM].
    ("53 HALF order is alphabetical, not Half.values()", PL,
     'const HALF: [&str; 2] = ["top", "bottom"];',
     'const HALF: [&str; 2] = ["bottom", "top"];', UNIT),
]


# `RUSTMC_FALSIFY_ONLY=49,50,51` runs a subset by leading number.
#
# **This is for re-checking injections added since the last full run, and the
# printed summary says which subset it was**, because a partial sweep quoted
# as a full one is the same defect class as a gate that stopped looking. It
# does not skip the controls: a subset run that cannot prove the suite was
# green either side proves nothing about the subset either.
def selected(injections):
    want = os.environ.get("RUSTMC_FALSIFY_ONLY", "").strip()
    if not want:
        return injections, "all"
    keep = {w.strip() for w in want.split(",") if w.strip()}
    out = [i for i in injections if i[0].split()[0] in keep]
    missing = keep - {i[0].split()[0] for i in out}
    if missing:
        raise SystemExit(f"RUSTMC_FALSIFY_ONLY names no such injection: {sorted(missing)}")
    return out, want


def run(cmd):
    r = subprocess.run(cmd, cwd=ROOT, shell=True, capture_output=True, text=True,
                       env={"PATH": f"{pathlib.Path.home()}/.cargo/bin:/usr/bin:/bin",
                            "HOME": str(pathlib.Path.home())})
    return r.returncode == 0


def write(path, text):
    path.write_text(text)
    # The stale-artifact defect: make the mtime unambiguously newer.
    now = time.time() + 1
    import os
    os.utime(path, (now, now))


def control(label, selectors):
    ok = all(run(s) for s in selectors)
    print(f"CONTROL {label}: {'GREEN' if ok else 'RED  <-- SWEEP IS INVALID'}")
    return ok


def restore_all(originals):
    """Put every file back, and say so loudly if one was left injected.

    **This exists because the first run of this sweep was killed mid-injection
    and left an injected bug sitting in `ruined_portal.rs`.** A sweep that can
    poison the tree it is checking is worse than no sweep: the next `cargo
    test` in that tree measures the injection, and nothing says so. The
    `finally` below runs on a normal exit, an exception and a SIGTERM.
    """
    dirty = []
    for path, text in originals.items():
        if path.read_text() != text:
            dirty.append(path.name)
        write(path, text)
    if dirty:
        print(f"RESTORED after an interrupted run: {', '.join(dirty)}")
    # **Verify, do not assume.** AGENT-BRIEF's inner loop is `rsync -az`, and
    # rsync's quick check is size plus mtime — an injection is a same-length
    # edit, so a poisoned file is exactly the shape rsync cannot see. The
    # restore above is an explicit write; this is the proof it took, by
    # content hash rather than by the write not having raised.
    bad = [
        path.name
        for path, text in originals.items()
        if hashlib.md5(path.read_bytes()).hexdigest()
        != hashlib.md5(text.encode()).hexdigest()
    ]
    if bad:
        print(f"RESTORE FAILED — STILL INJECTED: {', '.join(bad)}")
        print("DO NOT TRUST THIS TREE. Restore by explicit copy, not by rsync.")
        return False
    print("restore verified by md5: " + ", ".join(sorted(p.name for p in originals)))
    return True


def main():
    originals = {p: p.read_text() for p in (RP, TP, SS, PL)}
    chosen, subset = selected(INJECTIONS)
    selectors = sorted({i[4] for i in chosen})

    def on_term(_sig, _frame):
        ok = restore_all(originals)
        print("KILLED — sources restored, verdicts above are incomplete")
        sys.exit(3 if ok else 4)

    signal.signal(signal.SIGTERM, on_term)
    signal.signal(signal.SIGINT, on_term)

    if not control("before", selectors):
        print("refusing to run: the control is not green before any injection")
        return 2

    caught, missed, noop = 0, [], []
    try:
        caught, missed, noop = sweep(originals, chosen)
    finally:
        # An exception between injecting and restoring would otherwise leave
        # the bug in the tree for the next `cargo test` to measure.
        restored = restore_all(originals)

    if not restored:
        print("refusing to report: the tree is still poisoned")
        return 2
    ok_after = control("after", selectors)
    print()
    print(f"SWEEP: {caught}/{len(chosen)} caught" + ("" if subset == "all" else f"  (SUBSET {subset} of {len(INJECTIONS)} — NOT a full sweep)"))
    if missed:
        print("NOT CAUGHT:")
        for m in missed:
            print("   ", m)
    if noop:
        print("NO-OP (pattern missing — these measured nothing):")
        for m in noop:
            print("   ", m)
    if not ok_after:
        print("THE CONTROL AFTER IS RED — every verdict above is suspect")
        return 2
    return 0


def sweep(originals, chosen):
    caught, missed, noop = 0, [], []
    for name, path, find, repl, sel in chosen:
        src = originals[path]
        if find not in src:
            noop.append(name)
            print(f"{name:58} NO-OP  <-- pattern not found, injection did nothing")
            continue
        write(path, src.replace(find, repl, 1))
        red = not run(sel)
        write(path, src)  # restore, with a fresh mtime
        if red:
            caught += 1
            print(f"{name:58} RED")
        else:
            missed.append(name)
            print(f"{name:58} GREEN  <-- NOT CAUGHT")
    return caught, missed, noop


sys.exit(main())
