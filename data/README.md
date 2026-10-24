# data/ — extracted vanilla data (NOT committed)

`data/vanilla/` holds the 1,501 jar-extracted JSON files `rustmc-data`
loads (registries, tags, loot tables, worldgen configs). It is
**gitignored on purpose**: the content is derived from Mojang's jar, and
this repo goes public at launch — same policy as `vanilla-src/` (never
commit Mojang-derived material).

Provenance of the current local copy:

- Source: `~/minecom/src/main/resources/vanilla` @ minecom commit `a1d9d40`
  (2026-07-19), game version 26.2.
- Regenerate: `python3 ~/minecom/scripts/extract_vanilla_data.py --jar
  <vanilla 26.2 server jar> --out ~/rustmc/data/vanilla` (extractor has
  `--validate`; run it after regen).

On a version bump, the 48h-SLA pipeline (CI.md) regenerates this dir as its
second step, right after fetching the new jar.
