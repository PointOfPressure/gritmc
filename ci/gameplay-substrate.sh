#!/usr/bin/env bash
# gameplay-substrate.sh — the game rules the gameplay corpus runs under, in ONE
# place, for BOTH servers.
#
# Sourced by `ci/t2-gameplay.sh` and `ci/t2-gameplay-subset.sh`. Sets
# `SUBSTRATE_GAMERULES`.
#
# # WHY THIS FILE EXISTS — the same defect twice, in the same pair of scripts
#
# `ci/gameplay-difficulty.sh`'s header records that the corpus booted vanilla at
# `peaceful` and this server at its own default. This is that defect's sibling,
# found in the same hour and living one function away:
#
#   * **`ci/t2-gameplay.sh` quiets both servers.** It sends the four rules below
#     to vanilla through `gamerule_or_die`, and then to this server through
#     `rustmc_gamerule_or_die`, over the `--console` fifo. Its own comment says
#     why the second half exists: this server's `random_tick_speed` default is
#     vanilla's 3, so an un-quieted subject "would random-tick every rig's grass
#     at speed 3 for all ninety-two scenarios that are not `780`, reproducing
#     the exact flap `SUBSTRATE_GAMERULES` was written to stop, on the other
#     server".
#   * **`ci/t2-gameplay-subset.sh` quiets only vanilla.** It sent the same four
#     rules to the reference and nothing at all to the subject — it passed no
#     `--console` to `rustmc-server`, so it could not have. The subject
#     random-ticked, advanced its clock and cycled its weather through every
#     subset run this project has taken.
#
# That is the exact asymmetry the subset script's own header forbids: "a red
# observed here is evidence about a red observed there" only if the two
# instruments run the same environment. And it is the asymmetry the corpus's
# `crate::randomtick` module doc once described as permanent and was fixed for
# in the gate — the fix landed in one of the two files.
#
# **And the list was written out twice**, which is how it could drift. The gate
# declares it in an array; the subset had it as a `for r in …; case …` loop
# with the values inline. The reference cache's `harness` component is the hash
# of `ci/t2-gameplay.sh` alone, so editing the gate's list correctly invalidates
# the cache and silently left the subset on the old one.
#
# # THE VALUES, AND WHY THEY ARE 26.2 IDS
#
# `advance_time`, `spawn_mobs`, `random_tick_speed`, `advance_weather`. Game
# rules became a registry in 26.2 with snake_case namespaced ids, and
# `doDaylightCycle` / `doMobSpawning` / `randomTickSpeed` / `doWeatherCycle` are
# answered `Incorrect argument for command` and ignored. `docs/AGENT-BRIEF.md`
# records that being discovered twice, a week apart, because the correction
# lived only in the file it corrected. One array, one file, is the general
# answer to that.
#
# `ci/gameplay-substrate-falsify.sh` is its gate: it asserts both scripts read
# this array rather than a literal, and that each delivers it to BOTH servers.

SUBSTRATE_GAMERULES=(advance_time=false spawn_mobs=false random_tick_speed=0 advance_weather=false)
