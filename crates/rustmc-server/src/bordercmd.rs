//! `/worldborder`'s executors: `WorldBorderCommand`'s seven `int` methods,
//! the five packets its listener broadcasts, and the nine reply components.
//!
//! [`crate::worldborder`] is the transcription — the border, its two extents
//! and the six packet encodings. This module is the **wiring**: which of those
//! packets goes on the wire when, to whom, and what the caller is told.
//!
//! # The five-of-seven rule, and it is not an omission
//!
//! `PlayerList.addWorldborderListener` installs `PlayerList$1`, whose seven
//! methods are read from the jar and are **not** seven broadcasts:
//!
//! | listener method | packet |
//! | --- | --- |
//! | `onSetSize` | `set_border_size` |
//! | `onLerpSize` | `set_border_lerp_size` |
//! | `onSetCenter` | `set_border_center` |
//! | `onSetWarningTime` | `set_border_warning_delay` |
//! | `onSetWarningBlocks` | `set_border_warning_distance` |
//! | `onSetDamagePerBlock` | **nothing — the method body is empty** |
//! | `onSetSafeZone` | **nothing — the method body is empty** |
//!
//! So `/worldborder damage amount` and `/worldborder damage buffer` change
//! server state, answer the caller, and put **no packet on the wire at all**.
//! A port that broadcast something there would be adding a packet vanilla
//! does not send, and `bordercmd::tests::damage_sends_no_packet` is the
//! assertion that keeps it that way.
//!
//! # `tick()` broadcasts nothing either, and the brief for this work said it did
//!
//! `WorldBorder.tick()` is three instructions — `extent = extent.update()` —
//! and reaches no listener (`WorldBorder.class`, `public void tick()`, offsets
//! 0..13; the only `BorderChangeListener` call sites in the whole class are in
//! `setCenter`, `setSize`, `lerpSizeBetween`, `setSafeZone`,
//! `setDamagePerBlock`, `setWarningTime` and `setWarningBlocks`). The whole
//! shrink animation is the client interpolating the single
//! `set_border_lerp_size` it was sent. Ticking still has to happen on the
//! server, because `getSize()` and the `initialize_border` a *later* joiner
//! receives both read the extent — see [`crate::tick`]'s border step — but it
//! is not a per-tick broadcast and building one would put ~20 packets a second
//! on a channel `tools/oracle-runner` digests whole.
//!
//! # The replies are `String.format`, and Java's is not Rust's
//!
//! Every one of the nine is `Component.translatable(key, args)` where all but
//! one argument is a `String` produced by
//! `String.format(Locale.ROOT, "%.Nf", value)`. Java's `%f` rounds
//! **half-up on the shortest round-trip decimal repr**, Rust's `{:.N}` rounds
//! **half-to-even on the exact binary value**, and they disagree on inputs a
//! player types: `%.1f` of `0.15` is `0.2` in Java and `0.1` in Rust, `%.2f`
//! of `0.125` is `0.13` and `0.12`. [`java_format_f`] is the transcription and
//! `crates/rustmc-server/golden/commands.json`'s `worldborder*` replies are
//! vanilla's own bytes for values chosen to separate the two.

use std::io;

use rustmc_proto::ids;
use rustmc_world::nbt::Nbt;

use crate::commands::{translatable, Arg};
use crate::worldborder::{self, packets, WorldBorder};

/// `WorldBorder.MAX_SIZE`, the `set`/`add` upper bound.
const MAX_SIZE: f64 = worldborder::MAX_SIZE;

/// `WorldBorder.MAX_CENTER_COORDINATE`, the `center` bound.
const MAX_CENTER: f64 = worldborder::MAX_CENTER_COORDINATE;

/// Which of `WorldBorderCommand`'s `ERROR_*` a call refused with.
///
/// Vanilla raises these as `SimpleCommandExceptionType`s, which reach a client
/// through `Commands.finishParsing`'s failure components — the path
/// [`crate::commands::failure_components`] renders. They are **not** rendered
/// here: a `SimpleCommandExceptionType.create()` raised inside an *executor*
/// takes a different route from a parse failure (`Commands.executeCommand`'s
/// catch, not `finishParsing`'s), that route is not ported, and inventing a
/// component for it would put wrong bytes on the chat channel. The variant is
/// carried so the day it is ported the caller already knows which message it
/// owes, and so the refusals can be asserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderError {
    /// `ERROR_SAME_CENTER` — `commands.worldborder.center.failed`.
    SameCenter,
    /// `ERROR_TOO_FAR_OUT` — `commands.worldborder.set.failed.far`.
    TooFarOut,
    /// `ERROR_SAME_SIZE` — `commands.worldborder.set.failed.nochange`.
    SameSize,
    /// `ERROR_TOO_SMALL` — `commands.worldborder.set.failed.small`.
    TooSmall,
    /// `ERROR_TOO_BIG` — `commands.worldborder.set.failed.big`.
    TooBig,
    /// `ERROR_SAME_WARNING_TIME` — `commands.worldborder.warning.time.failed`.
    SameWarningTime,
    /// `ERROR_SAME_WARNING_DISTANCE` —
    /// `commands.worldborder.warning.distance.failed`.
    SameWarningDistance,
    /// `ERROR_SAME_DAMAGE_BUFFER` — `commands.worldborder.damage.buffer.failed`.
    SameDamageBuffer,
    /// `ERROR_SAME_DAMAGE_AMOUNT` — `commands.worldborder.damage.amount.failed`.
    SameDamageAmount,
}

/// One packet the border listener asks to be broadcast, as `(id, body)`.
pub type BorderPacket = (i32, Vec<u8>);

/// What an executor produced: the packets `PlayerList$1` would broadcast, and
/// the component `sendSuccess` would deliver.
///
/// Both are returned rather than sent, so that every one of the tests below
/// reads exactly what a client would and none of them needs a socket.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// Empty for the two `damage` sub-commands, one packet for every other.
    pub packets: Vec<BorderPacket>,
    /// The `sendSuccess` component.
    pub reply: Nbt,
    /// The `int` the Brigadier `Command` returns. Unobservable on the wire
    /// today — nothing here reads a command's result — and transcribed because
    /// `/execute store result` is the thing that will.
    pub result: i32,
}

/// `String.format(Locale.ROOT, "%.<digits>f", value)`.
///
/// # Java's `%f` is not Rust's `{:.N}` and the difference is reachable
///
/// Java's `Formatter` for `%f` does **not** expand the double exactly. It
/// takes `FloatingDecimal`'s shortest round-trip digit string — the same
/// digits `Double.toString` prints — and applies **`RoundingMode.HALF_UP`** to
/// that, padding with zeros when the repr is shorter than the request. Rust's
/// `{:.N}` expands the double exactly and rounds half-to-**even**. Two
/// consequences, both reachable from a typed command:
///
/// * `%.1f` of `0.15`: the shortest repr is `0.15`, half-up gives `0.2`. The
///   exact value is `0.1499999999999999944…`, so Rust gives `0.1`.
/// * `%.2f` of `0.125`: exactly representable, so both see a tie — Java rounds
///   up to `0.13`, Rust rounds to even and gives `0.12`.
///
/// A `f32` argument is widened to `f64` first, which is what
/// `Formatter$FormatSpecifier.print(float)` does (`print((double) value, l)`),
/// so `%.2f` of the float `0.1f` is `0.10` and not `0.1000000015`.
///
/// # Errors
/// None — a non-finite value cannot reach here from these commands, and is
/// formatted the way Rust formats it rather than being special-cased into a
/// guess at Java's `Infinity`/`NaN` spelling.
#[must_use]
pub fn java_format_f(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    // Rust's `Display` for `f64` is the shortest round-trip decimal and never
    // uses an exponent, which is the same digit string Java's
    // `FloatingDecimal` hands its formatter.
    let shortest = format!("{value}");
    let (sign, body) = match shortest.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", shortest.as_str()),
    };
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    if frac_part.len() <= digits {
        // Shorter than asked for: pad, never round.
        if digits == 0 {
            return format!("{sign}{int_part}");
        }
        let pad = "0".repeat(digits - frac_part.len());
        return format!("{sign}{int_part}.{frac_part}{pad}");
    }
    // Half-up on the digit string. Everything from `digits` on decides it, and
    // a first dropped digit of 5 rounds up regardless of what follows — that
    // is what HALF_UP means and it is why `0.15` at one digit is `0.2`.
    let keep: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes().take(digits))
        .collect();
    let round_up = frac_part.as_bytes()[digits] >= b'5';
    let mut out = keep;
    if round_up {
        let mut i = out.len();
        loop {
            if i == 0 {
                out.insert(0, b'1');
                break;
            }
            i -= 1;
            if out[i] == b'9' {
                out[i] = b'0';
            } else {
                out[i] += 1;
                break;
            }
        }
    }
    let s = String::from_utf8(out).unwrap_or_default();
    let split = s.len() - digits;
    if digits == 0 {
        format!("{sign}{s}")
    } else {
        format!("{sign}{}.{}", &s[..split], &s[split..])
    }
}

/// `setCenter(source, Vec2)`.
///
/// The bound tested is `MAX_CENTER_COORDINATE` — 2.9999984E7, **half** the
/// size bound — and it is tested on `Math.abs` of each component
/// independently, before `setCenter` is called (offsets 41..77). The equality
/// test that raises `ERROR_SAME_CENTER` compares the border's `double` centre
/// against the argument widened from `float`, so a centre set from a command
/// can never be "the same" as one carrying more precision than a float holds.
///
/// # Errors
/// [`BorderError::SameCenter`], [`BorderError::TooFarOut`].
pub fn set_center(b: &mut WorldBorder, x: f32, z: f32) -> Result<Outcome, BorderError> {
    if b.center_x() == f64::from(x) && b.center_z() == f64::from(z) {
        return Err(BorderError::SameCenter);
    }
    if f64::from(x.abs()) > MAX_CENTER || f64::from(z.abs()) > MAX_CENTER {
        return Err(BorderError::TooFarOut);
    }
    b.set_center(f64::from(x), f64::from(z));
    let mut body = Vec::new();
    packets::SetBorderCenter::of(b)
        .write(&mut body)
        .unwrap_or_default();
    Ok(Outcome {
        packets: vec![(ids::play::cb::SET_BORDER_CENTER, body)],
        reply: translatable(
            "commands.worldborder.center.success",
            &[
                Arg::Text(java_format_f(f64::from(x), 2)),
                Arg::Text(java_format_f(f64::from(z), 2)),
            ],
        ),
        // `iconst_0; ireturn` — this one always returns zero.
        result: 0,
    })
}

/// `setSize(source, newSize, timeTicks)`, which is both `/worldborder set` and
/// (through `getSize() + distance`) `/worldborder add`.
///
/// The three refusals are ordered and the order matters: `ERROR_SAME_SIZE`
/// first, so `/worldborder set 0` on a border already at 0 — impossible — is
/// moot, but `/worldborder add 0` is `nochange` rather than `small`.
///
/// `time > 0` takes the lerp branch and sends `set_border_lerp_size`;
/// `time == 0` calls `setSize` and sends `set_border_size`. The reply differs
/// three ways, not two: immediate, shrink and grow.
///
/// The `begin` argument `lerpSizeBetween` receives is `level.getGameTime()`,
/// and it is used only by `getLerpSpeed`; the countdown itself is
/// `lerpProgress`, initialised to the duration. See [`crate::worldborder`].
///
/// # Errors
/// [`BorderError::SameSize`], [`BorderError::TooSmall`], [`BorderError::TooBig`].
pub fn set_size(
    b: &mut WorldBorder,
    new_size: f64,
    time_ticks: i64,
    game_time: i64,
) -> Result<Outcome, BorderError> {
    let old = b.size();
    if old == new_size {
        return Err(BorderError::SameSize);
    }
    if new_size < 1.0 {
        return Err(BorderError::TooSmall);
    }
    if new_size > MAX_SIZE {
        return Err(BorderError::TooBig);
    }
    let sized = java_format_f(new_size, 1);
    let (packet_id, reply) = if time_ticks > 0 {
        b.lerp_size_between(old, new_size, time_ticks, game_time);
        let key = if new_size > old {
            "commands.worldborder.set.grow"
        } else {
            "commands.worldborder.set.shrink"
        };
        (
            ids::play::cb::SET_BORDER_LERP_SIZE,
            translatable(
                key,
                &[
                    Arg::Text(sized),
                    Arg::Text(format_ticks_to_seconds(time_ticks)),
                ],
            ),
        )
    } else {
        b.set_size(new_size);
        (
            ids::play::cb::SET_BORDER_SIZE,
            translatable("commands.worldborder.set.immediate", &[Arg::Text(sized)]),
        )
    };
    let mut body = Vec::new();
    if packet_id == ids::play::cb::SET_BORDER_LERP_SIZE {
        packets::SetBorderLerpSize::of(b)
            .write(&mut body)
            .unwrap_or_default();
    } else {
        packets::SetBorderSize::of(b)
            .write(&mut body)
            .unwrap_or_default();
    }
    Ok(Outcome {
        packets: vec![(packet_id, body)],
        reply,
        // `dload_1; dload 7; dsub; d2i` — the DIFFERENCE, narrowed the way
        // Java narrows a double to an int (toward zero, saturating).
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`d2i` is a saturating truncation and so is Rust's `as`"
        )]
        result: (new_size - old) as i32,
    })
}

/// `getSize(source)` — the one executor that sends `sendSuccess(..., false)`,
/// so it does not reach the admin broadcast.
///
/// The returned int is `Mth.floor(size + 0.5)`, which is a *rounded* size and
/// not the truncation the neighbouring executors use.
#[must_use]
pub fn get_size(b: &WorldBorder) -> Outcome {
    let size = b.size();
    Outcome {
        packets: Vec::new(),
        reply: translatable(
            "commands.worldborder.get",
            &[Arg::Text(java_format_f(size, 0))],
        ),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`Mth.floor(double)` is a floor narrowed to int, which is what this is"
        )]
        result: (size + 0.5).floor() as i32,
    }
}

/// `setWarningTime(source, ticks)`.
///
/// # Errors
/// [`BorderError::SameWarningTime`].
pub fn set_warning_time(b: &mut WorldBorder, ticks: i32) -> Result<Outcome, BorderError> {
    if b.warning_time() == ticks {
        return Err(BorderError::SameWarningTime);
    }
    b.set_warning_time(ticks);
    let mut body = Vec::new();
    packets::SetBorderWarningDelay::of(b)
        .write(&mut body)
        .unwrap_or_default();
    Ok(Outcome {
        packets: vec![(ids::play::cb::SET_BORDER_WARNING_DELAY, body)],
        // The one reply whose argument is a formatted STRING where the
        // neighbouring `warning distance` reply's is a bare int.
        reply: translatable(
            "commands.worldborder.warning.time.success",
            &[Arg::Text(format_ticks_to_seconds(i64::from(ticks)))],
        ),
        result: ticks,
    })
}

/// `setWarningDistance(source, blocks)`.
///
/// # Errors
/// [`BorderError::SameWarningDistance`].
pub fn set_warning_distance(b: &mut WorldBorder, blocks: i32) -> Result<Outcome, BorderError> {
    if b.warning_blocks() == blocks {
        return Err(BorderError::SameWarningDistance);
    }
    b.set_warning_blocks(blocks);
    let mut body = Vec::new();
    packets::SetBorderWarningDistance::of(b)
        .write(&mut body)
        .unwrap_or_default();
    Ok(Outcome {
        packets: vec![(ids::play::cb::SET_BORDER_WARNING_DISTANCE, body)],
        // `Integer.valueOf(i)` — a primitive translation argument, NOT a
        // formatted string. It is the only one of the nine.
        reply: translatable(
            "commands.worldborder.warning.distance.success",
            &[Arg::Int(blocks)],
        ),
        result: blocks,
    })
}

/// `setDamageAmount(source, damagePerBlock)` — **no packet**.
///
/// # Errors
/// [`BorderError::SameDamageAmount`].
pub fn set_damage_amount(b: &mut WorldBorder, amount: f32) -> Result<Outcome, BorderError> {
    if b.damage_per_block() == f64::from(amount) {
        return Err(BorderError::SameDamageAmount);
    }
    b.set_damage_per_block(f64::from(amount));
    Ok(Outcome {
        packets: Vec::new(),
        reply: translatable(
            "commands.worldborder.damage.amount.success",
            &[Arg::Text(java_format_f(f64::from(amount), 2))],
        ),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`f2i` is a saturating truncation and so is Rust's `as`"
        )]
        result: amount as i32,
    })
}

/// `setDamageBuffer(source, distance)` — the safe zone, and **no packet**.
///
/// # Errors
/// [`BorderError::SameDamageBuffer`].
pub fn set_damage_buffer(b: &mut WorldBorder, distance: f32) -> Result<Outcome, BorderError> {
    if b.safe_zone() == f64::from(distance) {
        return Err(BorderError::SameDamageBuffer);
    }
    b.set_safe_zone(f64::from(distance));
    Ok(Outcome {
        packets: Vec::new(),
        reply: translatable(
            "commands.worldborder.damage.buffer.success",
            &[Arg::Text(java_format_f(f64::from(distance), 2))],
        ),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`f2i` is a saturating truncation and so is Rust's `as`"
        )]
        result: distance as i32,
    })
}

/// `formatTicksToSeconds(long)` — `String.format(Locale.ROOT, "%.2f", t / 20.0)`.
#[must_use]
pub fn format_ticks_to_seconds(ticks: i64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "`l2d` is Java's own widening here and loses the same bits"
    )]
    let seconds = ticks as f64 / 20.0;
    java_format_f(seconds, 2)
}

/// Why a `/worldborder` could not run.
///
/// Two kinds, and keeping them apart is the point: a [`BorderError`] is
/// **vanilla refusing** — the border is already that size, the centre is
/// already there — and every other variant is **this server unable to read
/// its own tree**, which is a programming error reported as one rather than
/// answered with an invented message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// One of `WorldBorderCommand`'s nine `ERROR_*`.
    Refused(BorderError),
    /// The path resolved but an argument did not re-read. Unreachable through
    /// `Tree::resolve`, which already ran the same parser to accept it.
    Unreadable(&'static str),
    /// A path through the `worldborder` root that this function does not map.
    /// `commands::tests::every_executable_node_has_a_reply` is what keeps this
    /// unreachable, and it is a refusal rather than an `unreachable!()`
    /// because this runs on a connection thread, where a panic reaches a
    /// reader as a dropped socket.
    NoSuchPath(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(e) => write!(
                f,
                "{e:?} — vanilla's message for an executor refusal is not ported, so silence"
            ),
            Self::Unreadable(name) => write!(f, "the {name} argument did not re-read"),
            Self::NoSuchPath(p) => write!(f, "{p} is in the tree with no executor"),
        }
    }
}

/// Map a resolved `/worldborder` command onto its executor.
///
/// Shared by [`crate::commands`] (a player typed it) and [`crate::console`]
/// (the process's own stdin, which `MinecraftServer.createCommandSourceStack`
/// gives unconditional permission and no player). Neither the lock nor the
/// broadcast is here: the two callers answer different audiences.
///
/// # `add` is `set` with the current size added, and that is vanilla's own shape
///
/// `lambda$register$1`/`$0` call `setSize(source, border.getSize() + distance,
/// time)`. So `/worldborder add 0` is `ERROR_SAME_SIZE` rather than a no-op
/// success, and `/worldborder add -20` shrinks — which is why the argument's
/// declared minimum is negative where a "distance" reads as positive.
///
/// `source_pos` is the `(x, z)` a relative `~` centre resolves against —
/// `CommandSourceStack.getPosition()`, which for a console source is the
/// level's spawn and for a player is the player.
///
/// # Errors
/// [`RunError`], whose three variants say which of the two kinds it is.
pub fn run(
    resolved: &crate::commandtree::Resolved,
    border: &mut WorldBorder,
    source_pos: (f64, f64),
    game_time: i64,
) -> Result<Outcome, RunError> {
    /// `DoubleArgumentType.getDouble` — the token re-read through the same
    /// reader that accepted it.
    fn double(text: &str) -> Option<f64> {
        crate::cmdreader::Reader::new(text).read_double().ok()
    }
    /// `FloatArgumentType.getFloat`.
    fn float(text: &str) -> Option<f32> {
        crate::cmdreader::Reader::new(text).read_float().ok()
    }
    /// `IntegerArgumentType.getInteger`.
    fn int(text: &str) -> Option<i32> {
        crate::cmdreader::Reader::new(text).read_int().ok()
    }
    /// `TimeArgument`'s value at the minimum every `/worldborder` time node
    /// declares.
    fn ticks(text: &str) -> Option<i64> {
        let mut r = crate::cmdreader::Reader::new(text);
        crate::commandtree::time_ticks(&mut r, 0)
            .ok()
            .map(i64::from)
    }

    let outcome = match resolved.path.as_slice() {
        ["worldborder", op @ ("add" | "set"), "distance", rest @ ..] => {
            let distance = resolved
                .arg("distance")
                .and_then(double)
                .ok_or(RunError::Unreadable("distance"))?;
            let target = if *op == "add" {
                border.size() + distance
            } else {
                distance
            };
            let time = match rest {
                ["time"] => resolved
                    .arg("time")
                    .and_then(ticks)
                    .ok_or(RunError::Unreadable("time"))?,
                _ => 0,
            };
            set_size(border, target, time, game_time)
        }
        ["worldborder", "center", "pos"] => {
            let text = resolved.arg("pos").ok_or(RunError::Unreadable("pos"))?;
            let mut r = crate::cmdreader::Reader::new(text);
            let (x, z) = crate::commandtree::vec2_value(&mut r, source_pos.0, source_pos.1)
                .map_err(|_| RunError::Unreadable("pos"))?;
            set_center(border, x, z)
        }
        ["worldborder", "damage", "amount", "damagePerBlock"] => {
            let v = resolved
                .arg("damagePerBlock")
                .and_then(float)
                .ok_or(RunError::Unreadable("damagePerBlock"))?;
            set_damage_amount(border, v)
        }
        ["worldborder", "damage", "buffer", "distance"] => {
            let v = resolved
                .arg("distance")
                .and_then(float)
                .ok_or(RunError::Unreadable("distance"))?;
            set_damage_buffer(border, v)
        }
        ["worldborder", "get"] => Ok(get_size(border)),
        ["worldborder", "warning", "distance", "distance"] => {
            let v = resolved
                .arg("distance")
                .and_then(int)
                .ok_or(RunError::Unreadable("distance"))?;
            set_warning_distance(border, v)
        }
        ["worldborder", "warning", "time", "time"] => {
            let v = resolved
                .arg("time")
                .and_then(ticks)
                .and_then(|t| i32::try_from(t).ok())
                .ok_or(RunError::Unreadable("time"))?;
            set_warning_time(border, v)
        }
        other => return Err(RunError::NoSuchPath(format!("{other:?}"))),
    };
    outcome.map_err(RunError::Refused)
}

/// Broadcast one border packet to every connection, which is
/// `PlayerList.broadcastAll(packet, dimension)` on a one-dimension server.
///
/// A send failure is dropped rather than propagated: a player whose socket has
/// gone is not a reason to fail the command for the seven who are still here,
/// and it is what `broadcastAll` does (it walks the list and each `send` is
/// independent).
pub fn broadcast(players: &crate::players::Players, packets: &[BorderPacket]) {
    for conn in players.connections() {
        for (id, body) in packets {
            let _: io::Result<()> = conn.send(*id, body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five values that separate Java's HALF_UP-on-the-shortest-repr from
    /// Rust's half-even-on-the-exact-value. Without this the formatter would
    /// be "obviously right" and wrong on every one of them.
    #[test]
    fn the_formatter_is_javas_and_not_rusts() {
        // Exactly representable ties: Java rounds up, Rust rounds to even.
        assert_eq!(java_format_f(0.125, 2), "0.13");
        assert_eq!(format!("{:.2}", 0.125), "0.12", "the control");
        assert_eq!(java_format_f(2.5, 0), "3");
        assert_eq!(format!("{:.0}", 2.5), "2", "the control");
        // A repr that reads as a tie but whose exact value is below it.
        assert_eq!(java_format_f(0.15, 1), "0.2");
        assert_eq!(format!("{:.1}", 0.15), "0.1", "the control");
        // Carry all the way out.
        assert_eq!(java_format_f(9.99, 1), "10.0");
        assert_eq!(java_format_f(0.999, 0), "1");
        // Padding, not rounding.
        assert_eq!(java_format_f(100.0, 1), "100.0");
        assert_eq!(java_format_f(100.0, 2), "100.00");
        assert_eq!(
            java_format_f(-3.5, 0),
            "-4",
            "half-up is away from zero on the digits"
        );
    }

    #[test]
    fn ticks_to_seconds_is_two_places_of_a_twentieth() {
        assert_eq!(format_ticks_to_seconds(0), "0.00");
        assert_eq!(format_ticks_to_seconds(20), "1.00");
        assert_eq!(format_ticks_to_seconds(1), "0.05");
        assert_eq!(format_ticks_to_seconds(7), "0.35");
    }

    /// The refusal order inside `setSize`, which is `nochange` before `small`.
    #[test]
    fn the_refusals_are_in_vanillas_order() {
        let mut b = WorldBorder::default();
        b.set_size(64.0);
        assert_eq!(set_size(&mut b, 64.0, 0, 0), Err(BorderError::SameSize));
        assert_eq!(set_size(&mut b, 0.5, 0, 0), Err(BorderError::TooSmall));
        assert_eq!(
            set_size(&mut b, MAX_SIZE + 1.0, 0, 0),
            Err(BorderError::TooBig)
        );
    }

    /// The two `damage` sub-commands put nothing on the wire. This is the
    /// assertion the module header exists for.
    #[test]
    fn damage_sends_no_packet() {
        let mut b = WorldBorder::default();
        let a = set_damage_amount(&mut b, 0.35).expect("changed");
        assert!(a.packets.is_empty(), "onSetDamagePerBlock's body is empty");
        let c = set_damage_buffer(&mut b, 1.25).expect("changed");
        assert!(c.packets.is_empty(), "onSetSafeZone's body is empty");
        // And they did change the border, so the emptiness is not because
        // nothing happened.
        // `f64::from(0.35f32)` is 0.3499999940395355, which is the value
        // vanilla stores too — `setDamagePerBlock(f2d)` — so the tolerance is
        // a float's and not a double's.
        assert!((b.damage_per_block() - 0.35).abs() < 1e-6);
        assert!(
            (b.safe_zone() - 1.25).abs() < f64::EPSILON,
            "1.25 is exact in both"
        );
    }

    /// `set` with a time takes the lerp packet and `set` without takes the
    /// size packet — the one branch a caller is most likely to collapse.
    #[test]
    fn a_timed_set_is_a_different_packet_from_an_immediate_one() {
        let mut b = WorldBorder::default();
        let now = set_size(&mut b, 64.0, 0, 0).expect("changed");
        assert_eq!(now.packets[0].0, ids::play::cb::SET_BORDER_SIZE);
        let later = set_size(&mut b, 12.0, 20, 100).expect("changed");
        assert_eq!(later.packets[0].0, ids::play::cb::SET_BORDER_LERP_SIZE);
        assert_eq!(b.status(), crate::worldborder::BorderStatus::Shrinking);
    }

    /// `lerpSizeBetween`'s `lerpBegin` reaches no packet.
    ///
    /// It is what lets [`crate::console`] pass zero for the game time without
    /// that being a stub: `set_border_lerp_size` carries `lerpProgress`, not
    /// `lerpEnd`, and `getLerpSpeed()` — the only reader of `lerpBegin` — is
    /// on no wire. The day a packet reads it, this test is the failure.
    #[test]
    fn lerp_begin_reaches_no_packet() {
        let mut early = WorldBorder::default();
        let mut late = WorldBorder::default();
        early.set_size(64.0);
        late.set_size(64.0);
        let a = set_size(&mut early, 12.0, 20, 0).expect("changed");
        let b = set_size(&mut late, 12.0, 20, 1_000_000).expect("changed");
        assert_eq!(a.packets, b.packets, "the begin time is not on the wire");
        // And the control: the two borders really do disagree about it.
        assert!(
            (early.lerp_speed() - late.lerp_speed()).abs() < f64::EPSILON,
            "the speed is a function of the SPAN, which is equal here"
        );
        let mut other = WorldBorder::default();
        other.set_size(64.0);
        let c = set_size(&mut other, 12.0, 40, 0).expect("changed");
        assert_ne!(a.packets, c.packets, "a different DURATION is on the wire");
    }

    /// `get` is the only executor that reports without changing anything, and
    /// its int is a rounded size rather than a truncated one.
    #[test]
    fn get_rounds_where_the_others_truncate() {
        let mut b = WorldBorder::default();
        b.set_size(63.6);
        let o = get_size(&b);
        assert!(o.packets.is_empty());
        assert_eq!(o.result, 64, "Mth.floor(size + 0.5)");
    }

    /// The centre bound is half the size bound, and it is per-component.
    #[test]
    fn the_centre_bound_is_max_center_coordinate() {
        let mut b = WorldBorder::default();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the bound as vanilla's float arg"
        )]
        let just_over = (MAX_CENTER as f32) * 1.001;
        assert_eq!(
            set_center(&mut b, just_over, 0.0),
            Err(BorderError::TooFarOut)
        );
        assert_eq!(
            set_center(&mut b, 0.0, just_over),
            Err(BorderError::TooFarOut),
            "the check is on each component, not on the distance"
        );
        let ok = set_center(&mut b, 137.0, -42.0).expect("changed");
        assert_eq!(ok.packets[0].0, ids::play::cb::SET_BORDER_CENTER);
        assert_eq!(
            set_center(&mut b, 137.0, -42.0),
            Err(BorderError::SameCenter)
        );
    }
}
