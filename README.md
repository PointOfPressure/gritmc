# gritmc

A Minecraft Java server written in Rust from scratch, aiming to behave exactly like vanilla. It is not a Paper or Bukkit fork, so Java plugins don't run on it.

## Vibecoded

I didn't write this code by hand. It was built with Claude Code (AI agents), with me directing. Treat it that way.

## Status

Parked. It runs and much of survival works, but it isn't finished and isn't meant for a real server yet.

Parity is measured against the unmodified vanilla jar rather than claimed. Over a 33×33 chunk overworld test area, its terrain can't be told apart from a vanilla run of the same seed. The details are in `docs/PARITY.md` and `docs/DETERMINISM.md`.

Building needs the `grit-plugins` repo beside this one, and that repo isn't public yet.

## Licence

Source-available, not open source: [PolyForm Noncommercial 1.0.0](LICENSE). Free to use, change and share for any non-commercial purpose. Commercial use needs permission.

Pull requests are merged only if you agree in writing that your contribution can be relicensed.
