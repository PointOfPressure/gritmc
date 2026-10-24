# w13 — packet compression, and why the obvious refactor fails

Everything this server sends is **uncompressed**. Vanilla negotiates
compression during login — `login/login_compression` carries a threshold,
default 256, and every frame after it in **both** directions is compressed.
Ours sends the packet never and the flag nowhere.

It matters for two reasons and neither is cosmetic. Nine chunks go out at
~4.3 KB each where vanilla would send a fraction of that, so any bandwidth
number this project quotes against vanilla or Paper would be measuring the
wrong thing. And a real client is perfectly happy without it, which is
exactly why it will go unnoticed until a benchmark says something flattering.

`rustmc-proto`'s framing already supports it: `write_frame` takes an
`Option<i32>` threshold and `read_frame` a `bool`. Every call site in
`rustmc-server` passes `None` and `false`.

## What was tried, and why it was reverted

A `Conn<S>` wrapper holding the stream and the threshold, with `send`/`recv`,
so the state lives next to the socket instead of being threaded through
fifteen signatures. That is still the right design — a frame written with the
wrong compression state is not recoverable, the client reads a length and
then garbage, and threading an `Option<i32>` by hand is how one call site
gets forgotten.

The mistake was doing the conversion with a scripted regex across five files
and 45 call sites at once. It produced `rustmc_proto::conn.send(...)` and 21
compile errors, and was reverted whole rather than patched — a half-converted
connection is worse than an unconverted one, because the compression state
would then be right in some paths and absent in others.

## How to do it

One file at a time, compiling between each, in dependency order so each step
is independently green:

1. `Conn<S>` in `lib.rs`, with `send`, `recv`, `set_threshold` and `get_mut`.
2. `login.rs` — and this is where the threshold is set, so it is the only
   file where the *order* matters: vanilla sends `login_compression`
   **before** `login_finished`, so the compression turns on mid-state and
   `login_finished` is already compressed.
3. `configuration.rs`, then `world_send.rs`, then `play.rs`.
4. `status.rs` last, and note it must **stay** uncompressed: the status
   exchange happens before login, so a server that compresses it is talking
   to a client that is not listening for compressed frames.

## How to know it worked

`ci/regdump.py` already handles compression on the client side — it reads the
threshold from `login_compression` and inflates thereafter, which is how it
talks to a real vanilla server. So the check is that it still completes login,
configuration and the play sequence against rustmc, with the chunk packets
arriving smaller than 4,311 bytes.


## Done, and measured

Landed as described: threshold threaded as a parameter, one file at a time,
compiling between each.

The test caught the exact failure this note warned about. The moment
compression went on, `a_login_completes_then_says_where_the_work_stops`
failed — its fixture still wrote `login_acknowledged` uncompressed. That is
the half-threaded bug, found by a test rather than by a client.

Verified against `ci/regdump.py`, which inflates only after reading a
threshold from `login_compression`, so the fact that it decodes at all is the
proof compression is on. And the wire benefit is measured rather than
asserted — the client now counts the on-wire frame lengths:

```text
  on the wire, whole session      20,326 bytes
  decompressed                   ~160,000
      registries                  121,719
      9 chunks at ~4,311           ~38,800
```

**About eight to one.** That is the number that would have been wrong in any
bandwidth comparison against vanilla or Paper, and it is the reason this was
worth doing before benchmarking rather than after.
