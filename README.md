# gritmc

<p align="center">
  <strong>Stopped. Archived. MIT, so copy what you like.</strong>
</p>

A Minecraft Java server written in Rust from scratch, aiming to behave exactly like vanilla.

## Status

**Stopped.** Other Rust Minecraft servers are growing faster than this one could, so it won't be worked on again. The repo is archived and kept public so anyone can take what's useful.

It runs and much of survival works, but it was never finished and isn't meant for a real server. It's not a Paper or Bukkit fork, so Java plugins don't run on it.

## Use these instead

| Project | What it is | Licence |
|---|---|---|
| [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) | Rust server, by far the most used (12k+ stars) | GPL-3.0 |
| [SteelMC](https://github.com/Steel-Foundation/SteelMC) | Rust server, focused on foundations and performance | AGPL-3.0 |
| [Horizon](https://github.com/ludovic111/horizon) | Java Edition rewritten in Rust, checked against vanilla | MIT |

## Copying it

MIT. Copy any of it into anything, including the projects above, as long as the copyright notice comes with it. You don't need to ask.

## Build

It needs [grit-plugins](https://github.com/PointOfPressure/grit-plugins) checked out next to it as `plugins`:

```bash
git clone https://github.com/PointOfPressure/gritmc
git clone https://github.com/PointOfPressure/grit-plugins plugins
cd gritmc
cargo build --release
```

The toolchain is pinned in `rust-toolchain.toml`.

To run it, `data/vanilla/` has to be filled with data extracted from the vanilla 26.2 server jar first. That data is Mojang's, so it isn't in the repo. `data/README.md` has the extractor command, and the `ci/dump-*.sh` scripts make the rest. Then:

```bash
cargo run --release -p rustmc-server -- run --port 25565
```

## How parity was measured

Against the unmodified vanilla jar, not claimed. Over a 33×33 chunk overworld test area, its terrain can't be told apart from a vanilla run of the same seed. The details are in `docs/PARITY.md` and `docs/DETERMINISM.md`. `docs/SERVER.md` lists what works and what doesn't.

```
crates/   the server: protocol, world, worldgen, entities, loot, behaviour
plugins/  first-party plugins built into the server
tools/    oracle runner and world differs used to measure parity
ci/       the gates and their falsifiers
docs/     parity, determinism, benchmarks, findings
```

<details>
<summary>About this project</summary>

Vibecoded. I didn't write this code by hand. It was built with Claude Code (AI agents), with me directing. Treat it that way.

</details>

---

[MIT](LICENSE). It was PolyForm Noncommercial until 25 Oct 2026.
