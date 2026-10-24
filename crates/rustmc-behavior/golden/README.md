# Golden pure-function vectors (exported from Minecom)

`{inputs, expected}` vectors for pure decision/math functions Minecom's
SelfTest covers (docs/ORACLE.md §4: "port as native Rust unit tests with test
vectors exported from the Java implementation").

## Provenance

- Source: Minecom (private, ~/minecom), branch `goldengen-dumps`,
  commit `1ec2f78`, pom 0.40.0, game/data version **26.2**
  (Minestom 2026.07.12-26.2).
- Java functions dumped: `Difficulty.effective`/`moonBrightness`
  (DifficultyInstance.calculateDifficulty), `data/Enchants.tableCosts`
  (EnchantmentHelper.getEnchantmentCost), `blocks/Beacons.validateEffects`
  (BeaconBlockEntity.validateEffects), `blocks/Conduits.effectRange`
  (ConduitBlockEntity.applyEffects).
- Dump run twice, `diff -r` byte-identical.

## Regeneration

From the Minecom worktree (branch `goldengen-dumps`):

```
mvn -q -o -DskipTests package
java -cp target/minecom.jar dev.pointofpressure.minecom.data.GoldenDump <outdir>
# files land in <outdir>/behavior/ — copy here
```

## Format

i64 inputs are JSON strings; f32 values are `{bits, repr}` (raw IEEE-754 bits
as an i32 string — compare bits, not repr).

- `difficulty_vectors.json`: `vectors` = 4 difficulties × 6 dayTimes × 5
  inhabitedTicks with the moonBrightness input recorded and `expected` =
  effective difficulty (f32). `moonBrightness` = the 8-phase table across 9
  day boundaries.
- `enchant_table_costs.json`: `{seed, bookcases, expected: [slot0, slot1,
  slot2]}` for 4 seeds × bookcases 0..15. RNG contract: one continuing
  `java.util.Random(seed)` stream, 2 `nextInt` draws per slot (6 total, NOT
  reseeded per slot); a computed cost `< slot+1` is emitted as 0 (hide-slot
  rule).
- `beacon_effects.json`: validateEffects truth table — 7 primaries × 8
  secondaries (incl. null) × levels 0..5.
- `conduit_range.json`: `effectRange(size) = size/7*16` (integer division)
  for size 0..56.

## Skipped (documented, not silent — ORACLE.md gap discipline)

- **Explosion math**: `Explosions.explode` requires a live Instance (rays
  sample real blocks); not exposed as a pure function, refactoring was out of
  scope. Parity comes via PlayTest.
- **Enchantment offer selection** (`Enchants.getEnchantmentList`): callable
  but needs the full vanilla-data load + enchantment-def serialization;
  deferred until the Rust enchanting port needs it (`tableCosts` covers the
  RNG-critical half).
