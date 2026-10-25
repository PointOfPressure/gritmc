# gritmc

A Minecraft Java server written in Rust from scratch, aiming to behave exactly like vanilla. It is not a Paper or Bukkit fork, so Java plugins don't run on it.

## Vibecoded

I didn't write this code by hand. It was built with Claude Code (AI agents), with me directing. Treat it that way.

## Status

Finished experiment, archived. It runs and much of survival works, but it was never completed and isn't meant for a real server.

For a Rust Minecraft server people are still building, see [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin). For a full Rust rewrite of the game, see [Horizon](https://github.com/ludovic111/horizon).

Parity is measured against the unmodified vanilla jar rather than claimed. Over a 33×33 chunk overworld test area, its terrain can't be told apart from a vanilla run of the same seed. The details are in `docs/PARITY.md` and `docs/DETERMINISM.md`.

Building needs the `grit-plugins` repo beside this one, and that repo is private, so this one doesn't build on its own.

## Licence

[MIT](LICENSE). It was PolyForm Noncommercial until 25 Oct 2026.
