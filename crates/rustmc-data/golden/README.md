# Golden fixed-table dumps (exported from Minecom)

Fixed tables Minecom's SelfTest asserts on, re-expressed as golden files
(docs/ORACLE.md §4: "dump the table from Minecom once, commit the JSON, assert
the Rust implementation reproduces it byte-for-byte").

## Provenance

- Source: Minecom (private, ~/minecom), branch `goldengen-dumps`,
  commit `1ec2f78`, pom 0.40.0, game/data version **26.2**
  (Minestom 2026.07.12-26.2).
- Java classes dumped: `data/MapColors` (verbatim port of decompiled
  MapColor/ARGB 26.2), `mobs/SulfurCubes.ARCHETYPES` (verbatim port of
  SulfurCubeArchetypes.bootstrap 26.2).
- Dump run twice, `diff -r` byte-identical.

## Regeneration

From the Minecom worktree (branch `goldengen-dumps`):

```
mvn -q -o -DskipTests package
java -cp target/minecom.jar dev.pointofpressure.minecom.data.GoldenDump <outdir>
# files land in <outdir>/data/ — copy here
```

## Format

- `map_colors.json`:
  - `colors`: the 62 base colors `{id, name, rgb}` (rgb = 24-bit int).
  - `brightness`: the 4 shades `{name, id, modifier}` (modifier = 180/220/
    255/135; ARGB channel scale = `channel * modifier / 255`).
  - `packed`: the full 64×4 grid `{id, brightness, packedId, argb}` —
    `packedId = id << 2 | brightness`; `argb` is the decoded pixel color as an
    **unsigned u32 string** (`MapColor.getColorFromPackedId`). Ids 62/63 have
    no color and exercise the NONE fallback (argb "0").
- `sulfur_archetypes.json`: the 12 archetypes in bootstrap order. Per
  archetype: `tag`, `buoyant`, `knockbackHorizontal`/`knockbackVertical`
  (f32 as `{bits, repr}` — compare bits), `explosion`
  (`{power, causesFire, fuse}` or null), `contactDamage`
  (`{damageType, amount, attributeToSource}` or null), and `modifiers` — the
  5 derived attribute modifiers `{attribute, id, operation, amount}` with
  amount as f64 `{bits, repr}`. Note the multiply modifiers store
  `amount − 1.0` under `ADD_MULTIPLIED_TOTAL` (vanilla's
  SulfurCubeArchetype.AttributeEntry.multiply convention) — the dump records
  the stored value.
