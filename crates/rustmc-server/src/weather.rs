//! The weather cycle: three timers, two levels, four `game_event` packets.
//!
//! Reference (26.2), `javap -p -c -constants` against the pinned
//! `server-26.2.jar`: `ServerLevel.advanceWeatherCycle`,
//! `ServerLevel.resetWeatherCycle`, `ServerLevel.prepareWeather`,
//! `ServerLevel.<clinit>`, `MinecraftServer.setWeatherParameters`,
//! `WeatherCommand`, `Level.{getRainLevel,setRainLevel,getThunderLevel,
//! setThunderLevel,isRaining,isThundering,canHaveWeather}`,
//! `PlayerList.{sendLevelInfo,broadcastAll}`,
//! `ClientboundGameEventPacket` and `WeatherData`.
//!
//! # Where the state lives in 26.2, which is not where it used to
//!
//! `ServerLevel.getWeatherData()` is
//! `this.server.getWeatherData()` — **one `WeatherData` for the whole
//! server**, not a per-level field and not `ServerLevelData`. It is a
//! `SavedData` with `SavedDataType` id `"weather"` and a `RecordCodec` over
//! five keys: `clear_weather_time`, `rain_time`, `thunder_time`, `raining`,
//! `thundering` (all from `WeatherData.<clinit>` and
//! `lambda$static$0`). The `raining`/`rainTime`/`thunderTime` fields that
//! older notes place in `level.dat` are not there in this version; anyone
//! porting the persistence side wants that file and those five key names.
//! Persisting it is **not** this module's — see "What this refuses" below.
//!
//! The *levels* are different again: `rainLevel`, `oRainLevel`,
//! `thunderLevel` and `oThunderLevel` are plain `float` fields on `Level`,
//! per level, and are not saved at all. They are rebuilt on load by
//! `ServerLevel.prepareWeather(WeatherData)`, which is four instructions:
//!
//! ```text
//!    0: if (!data.isRaining()) return;
//!    7: rainLevel = 1.0f;
//!   12: if (!data.isThundering()) return;
//!   19: thunderLevel = 1.0f;
//! ```
//!
//! so a world that was raining comes back at full rain immediately rather
//! than fading in. [`Weather::prepare`] is that method.
//!
//! # The cycle, `advanceWeatherCycle` offset by offset
//!
//! The method is one straight line with no loops. Read it as four parts.
//!
//! **Part 0, offsets 0-9.** `boolean wasRaining = isRaining()` is taken
//! **first**, before anything moves, and it is `Level.isRaining()` — the
//! *level* predicate, `getRainLevel(1.0f) > 0.2`, not
//! `WeatherData.isRaining()`. Then `if (!canHaveWeather()) goto 372`, which
//! skips the whole timer machine *and* both level ramps and lands on the
//! packet comparisons. In a dimension with no weather the two `o` fields
//! equal their live partners forever, so nothing is ever sent.
//!
//! **Part 1, offsets 12-261 — the timers, and only under the game rule.**
//! `GameRules.ADVANCE_WEATHER` (`advance_weather`, the 26.2 snake_case
//! spelling of what used to be `doWeatherCycle`) gates offsets 36..261. The
//! five values are read into locals *once* at 36-63, mutated as locals, and
//! written back at 233-261 in the order **thunderTime, rainTime,
//! clearWeatherTime, thundering, raining**. That write-back order is
//! recorded because it is observable through `WeatherData`'s dirty flag in
//! vanilla and because a port that writes them in source order is writing a
//! different method.
//!
//! The three timers are consulted in the order **clear, thunder, rain**, and
//! the first one wins outright:
//!
//! ```text
//!    65: if (clearWeatherTime > 0) {            // clear runs, nothing else does
//!    69:     clearWeatherTime--;
//!    72:     thunderTime  = thundering ? 0 : 1; //  <- 1, not a fresh sample
//!    84:     rainTime     = raining    ? 0 : 1;
//!    96:     thundering   = false;
//!    99:     raining      = false;
//!   102:     goto 233;                          //  <- skips BOTH arms below
//!   105: }
//!   105: if (thunderTime > 0) {                 // thunder timer
//!   110:     if (--thunderTime == 0) thundering = !thundering;
//!   133: } else {
//!   133:     thunderTime = (thundering ? THUNDER_DURATION : THUNDER_DELAY)
//!                             .sample(level.random);
//!   169: }
//!   169: if (rainTime > 0) {                    // rain timer, same shape
//!   174:     if (--rainTime == 0) raining = !raining;
//!   197: } else {
//!   197:     rainTime = (raining ? RAIN_DURATION : RAIN_DELAY).sample(level.random);
//!   233: }
//! ```
//!
//! Three consequences worth spelling out, because each is a place a
//! plausible reimplementation differs:
//!
//! * the clear arm sets the two timers to `0`/`1` rather than drawing. `1`
//!   means "expire next tick", so the tick after a clear period ends is the
//!   one that flips the corresponding flag through the `--x == 0` arm. It
//!   costs **no** RNG draw;
//! * the clear arm `goto`s past both other arms, so **at most one of the two
//!   `sample` calls happens per tick, and never both while clearing**;
//! * outside the clear arm, thunder is consulted before rain, so on a tick
//!   where both timers are exhausted the thunder sample is drawn **first**.
//!   The draw order is the reason [`Weather::advance_weather_cycle`] takes
//!   one `&mut` generator rather than two.
//!
//! **Part 2, offsets 262-371 — the levels.** Unconditional (given
//! `canHaveWeather`), and *not* under the game rule: with `advance_weather
//! false` the timers freeze but the levels still ramp toward whatever the
//! flags already say, which is what makes `/weather rain` work in a corpus
//! scenario running under the harness's `advance_weather false`.
//!
//! ```text
//!   262: oThunderLevel = thunderLevel;
//!   270: thunderLevel += data.isThundering() ? +0.01f : -0.01f;
//!   304: thunderLevel  = Mth.clamp(thunderLevel, 0.0f, 1.0f);
//!   317: oRainLevel    = rainLevel;
//!   325: rainLevel    += data.isRaining()    ? +0.01f : -0.01f;
//!   359: rainLevel     = Mth.clamp(rainLevel, 0.0f, 1.0f);
//! ```
//!
//! The step is the `float` literal `0.01f` at constant-pool #1260, used by
//! all four arms — one constant, not two. Thunder is stepped **before**
//! rain, and both read `WeatherData`'s flags rather than the level they are
//! chasing. 100 ticks (5 s) is a full ramp.
//!
//! **Part 3, offsets 372-557 — what goes on the wire.** Four sends, in this
//! order, each guarded:
//!
//! ```text
//!   372: if (oRainLevel    != rainLevel)    broadcastAll(RAIN_LEVEL_CHANGE(rainLevel),       dimension());
//!   412: if (oThunderLevel != thunderLevel) broadcastAll(THUNDER_LEVEL_CHANGE(thunderLevel), dimension());
//!   452: if (wasRaining != isRaining()) {
//!   460:     broadcastAll(wasRaining ? STOP_RAINING(0.0f) : START_RAINING(0.0f));
//!   509:     broadcastAll(RAIN_LEVEL_CHANGE(rainLevel));
//!   533:     broadcastAll(THUNDER_LEVEL_CHANGE(thunderLevel));
//!   557: }
//! ```
//!
//! **The two halves broadcast to different audiences and it is not a
//! typo.** The two level-change sends at 384 and 424 use
//! `PlayerList.broadcastAll(Packet, ResourceKey<Level>)`, which filters on
//! `player.level().dimension() == key`; the three inside the transition arm
//! use the one-argument `broadcastAll(Packet)`, which sends to every player
//! on the server whatever dimension they are in. This server has one
//! dimension so the two sets coincide, but [`Audience`] carries the
//! distinction so a second dimension cannot silently inherit the wrong one.
//!
//! **A rain transition therefore emits three packets in one tick, and the
//! first of them is a duplicate**: the level moved on this tick too, so the
//! guard at 372 already fired and `RAIN_LEVEL_CHANGE` goes out twice with
//! the same payload. That is vanilla, it is measurable on the wire, and a
//! port that "tidies" it is a port that fails the corpus.
//!
//! # `isRaining` is a level predicate with a threshold, not a flag
//!
//! `Level.isRaining()` is `canHaveWeather() && getRainLevel(1.0f) > 0.2`, and
//! `getRainLevel(p)` is `Mth.lerp(p, oRainLevel, rainLevel)`, so at `p = 1.0`
//! it is just `rainLevel`. `isThundering()` is the same shape at `> 0.9`
//! over `getThunderLevel(1.0f)`, and **that one is a product**:
//! `getThunderLevel(p) = Mth.lerp(p, oThunderLevel, thunderLevel) *
//! getRainLevel(p)`. Thunder is scaled by rain on the way out, so a client
//! joining mid-storm is told `thunderLevel * rainLevel`, not `thunderLevel`.
//!
//! Because the threshold is 0.2 and the step is 0.01, `wasRaining !=
//! isRaining()` fires on the **21st** tick of a ramp up (0.21 > 0.2) and on
//! the 80th tick of a ramp down from 1.0 (0.20 is not > 0.2). The transition
//! is not the tick the flag flipped; it is the tick the level crossed.
//!
//! # What a joining player gets
//!
//! `PlayerList.sendLevelInfo(player, level)`, offsets 55-142, after the
//! border/time/spawn packets:
//!
//! ```text
//!    55: if (level.isRaining()) {
//!    62:     send(START_RAINING, 0.0f);
//!    80:     send(RAIN_LEVEL_CHANGE,    level.getRainLevel(1.0f));
//!   102:     send(THUNDER_LEVEL_CHANGE, level.getThunderLevel(1.0f));
//!   124: }
//!   124: send(LEVEL_CHUNKS_LOAD_START, 0.0f);
//! ```
//!
//! Nothing is sent when it is not raining — no `STOP_RAINING`, no zero
//! levels — because a fresh client is already dry. [`Weather::join_events`]
//! is those three, and `crate::world_send::enter_world` already sends the
//! `LEVEL_CHUNKS_LOAD_START` that follows them.
//!
//! # What this module refuses, by name
//!
//! Each of these is somebody else's file or an unported subsystem, and
//! naming them is cheaper than a half-port:
//!
//! * **lightning.** `ServerLevel.tickThunder`, `findLightningRod`,
//!   `findLightningTargetAround` and `LightningBolt` — `tickThunder` needs
//!   a `PoiManager`, a `Heightmap`, `getSkyDarken` and an entity that ticks.
//!   None exists here. `crate::entities` declares two `EntityKind`s and
//!   neither is `lightning_bolt`;
//! * **persistence.** `WeatherData` is a `SavedData` and the save side is
//!   `persistence-2`'s. The five codec keys are recorded above so that lane
//!   does not have to re-derive them;
//! * **mob reactions to rain** — `Mob`'s branches on `level.isRainingAt`.
//!   There are no mobs (`crate::status`'s own note);
//! * **sky darkening.** `ServerLevel.tick` calls `updateSkyBrightness()`
//!   immediately after the weather block, and `Level.getSkyDarken` folds
//!   `rainLevel`/`thunderLevel` into the light value. That is
//!   `light-engine-2`'s: this server sends empty light masks and lets the
//!   client light the chunk;
//! * **precipitation** — `ServerLevel.tickPrecipitation`, ice and snow. See
//!   [`PRECIPITATION_DRAWS_PER_CHUNK_TICK`] for what is recorded and
//!   `docs/cold-start/weather.md` for the measurement that refused it.

use crate::gamerules::GameRules;
use rustmc_worldgen::rng::RandomSource;

/// `ClientboundGameEventPacket$Type.id`, from the class's `<clinit>`, which
/// constructs each `Type` with an `int` literal in declaration order.
///
/// Only the four this module sends are named. The ids are what
/// `ClientboundGameEventPacket.write` puts on the wire as a **byte**,
/// followed by the `float` param — `writeByte(event.id); writeFloat(param)`,
/// offsets 0-21, which is the two-field body
/// `crate::world_send::enter_world` already writes for id 13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Event {
    /// `<clinit>` offset 31, `iconst_1`.
    StartRaining = 1,
    /// `<clinit>` offset 42, `iconst_2`.
    StopRaining = 2,
    /// `<clinit>` offset 98, `bipush 7`.
    RainLevelChange = 7,
    /// `<clinit>` offset 110, `bipush 8`.
    ThunderLevelChange = 8,
}

/// Which `PlayerList.broadcastAll` overload a send uses.
///
/// `advanceWeatherCycle` uses both and the difference is invisible on a
/// one-dimension server, which is exactly why it is a type here rather than
/// a comment: the wrong one is undetectable until a second dimension exists,
/// and then it is a bug nothing in the corpus can localise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// `broadcastAll(Packet, ResourceKey<Level>)` — only players whose
    /// `level().dimension()` equals this level's. The two level-change sends
    /// at offsets 384 and 424.
    ThisDimension,
    /// `broadcastAll(Packet)` — every player on the server. The three sends
    /// inside the `wasRaining != isRaining()` arm, offsets 464, 509 and 533.
    EveryPlayer,
}

/// One packet the cycle decided to send: `(event, param, audience)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Send {
    pub event: Event,
    pub param: f32,
    pub audience: Audience,
}

impl Send {
    /// `ClientboundGameEventPacket.write`, offsets 0-21: `writeByte(id)` then
    /// `writeFloat(param)`. Two fields, five bytes, no varint.
    #[must_use]
    pub fn body(self) -> Vec<u8> {
        let mut b = Vec::with_capacity(5);
        b.push(self.event as u8);
        b.extend_from_slice(&self.param.to_be_bytes());
        b
    }
}

/// One tick of the weather cycle against the process-wide state, returning
/// the packets to broadcast.
///
/// This is the whole of what `crate::status`'s tick loop calls. It is here
/// rather than there so the lock ordering — generator, then level — lives in
/// one place: `dispatch_console` takes the same two in the same order, and
/// the console runs on its own thread.
#[must_use]
pub fn tick() -> Vec<Send> {
    let advance = advance_weather(GameRules::shared());
    let mut rng = level_random()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut weather = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    weather.advance_weather_cycle(advance, &mut *rng)
}

/// `UniformInt.of(min, max)` — `ServerLevel.<clinit>` builds all four.
///
/// `sample(random)` is `Mth.randomBetweenInclusive(random, min, max)`, whose
/// whole body is `random.nextInt(max - min + 1) + min` (offsets 0-13). One
/// draw, inclusive at both ends.
#[derive(Debug, Clone, Copy)]
pub struct UniformInt {
    pub min: i32,
    pub max: i32,
}

impl UniformInt {
    /// `IntProvider.sample`. Exactly one `nextInt` draw.
    pub fn sample(self, rng: &mut impl RandomSource) -> i32 {
        // `RandomSource::next_i32_between_inclusive` is that method, already
        // transcribed once in `rustmc_worldgen::rng`; calling it rather than
        // rewriting `nextInt(max - min + 1) + min` here keeps one copy of
        // vanilla's wrapping arithmetic.
        rng.next_i32_between_inclusive(self.min, self.max)
    }
}

/// `ServerLevel.RAIN_DELAY` — `UniformInt.of(12000, 180000)`, `<clinit>`
/// offsets 15-24. 10 minutes to 2.5 hours.
pub const RAIN_DELAY: UniformInt = UniformInt {
    min: 12_000,
    max: 180_000,
};
/// `ServerLevel.RAIN_DURATION` — `UniformInt.of(12000, 24000)`, offsets
/// 27-36.
pub const RAIN_DURATION: UniformInt = UniformInt {
    min: 12_000,
    max: 24_000,
};
/// `ServerLevel.THUNDER_DELAY` — `UniformInt.of(12000, 180000)`, offsets
/// 39-48. The same pair as [`RAIN_DELAY`] and a separate field; they are
/// equal in this version and are not the same constant.
pub const THUNDER_DELAY: UniformInt = UniformInt {
    min: 12_000,
    max: 180_000,
};
/// `ServerLevel.THUNDER_DURATION` — `UniformInt.of(3600, 15600)`, offsets
/// 51-60. Three minutes to thirteen.
pub const THUNDER_DURATION: UniformInt = UniformInt {
    min: 3_600,
    max: 15_600,
};

/// The level step, `ldc_w #1260 // float 0.01f`, used by all four ramp arms
/// of `advanceWeatherCycle`.
pub const LEVEL_STEP: f32 = 0.01;

/// `Level.isRaining`'s threshold — `ldc2_w // double 0.2d`, compared with
/// `dcmpl` against the `float` level widened by `f2d`. The comparison is
/// **strictly greater** and happens in `double`, so it is written that way
/// here rather than as an `f32` compare.
pub const RAINING_ABOVE: f64 = 0.2;

/// `Level.isThundering`'s threshold — `ldc2_w // double 0.9d`, same shape.
pub const THUNDERING_ABOVE: f64 = 0.9;

/// `ServerLevel.tickChunk`'s ice-and-snow arm, offsets 32-76: `for (i = 0; i
/// < randomTickSpeed; i++) if (random.nextInt(48) == 0)
/// tickPrecipitation(getBlockRandomPos(minX, 0, minZ, 15))`.
///
/// **The bound is 48 and the loop count is `randomTickSpeed`**, which
/// `ServerChunkCache.tickChunks` reads from `GameRules.RANDOM_TICK_SPEED`
/// and passes as `tickChunk`'s second parameter. This is recorded here
/// because the brief that produced this module asserted the cadence was
/// `randomTickSpeed`-*independent* and the bytecode says it is not: at the
/// default speed of 3 a chunk attempts precipitation 3 times a tick with
/// probability 1/48 each, so 0.0625 attempts per chunk-tick.
/// `crate::randomtick`'s `PRECIPITATION_ATTEMPTS_PER_TICK` records the same
/// loop from the other side.
pub const PRECIPITATION_DRAWS_PER_CHUNK_TICK: i32 = 48;

/// `WeatherData` — the server-wide saved weather state, five fields.
///
/// Field order and names are the class's own (`clearWeatherTime`, `rainTime`,
/// `thunderTime`, `raining`, `thundering`); the codec keys are the
/// snake_case forms listed in this module's header. A default-constructed
/// `WeatherData` (the no-arg constructor, which sets nothing) leaves every
/// field at Java's zero value, which is what a fresh world starts from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeatherData {
    pub clear_weather_time: i32,
    pub rain_time: i32,
    pub thunder_time: i32,
    pub raining: bool,
    pub thundering: bool,
}

/// The four `float` fields on `Level` beside the saved data, and the saved
/// data itself.
///
/// One instance is one level. `canHaveWeather` is carried as a field rather
/// than recomputed because its inputs — `dimensionType().hasSkyLight()`,
/// `!hasCeiling()` and `dimension() != END` — are all per-level constants
/// this server does not otherwise model.
#[derive(Debug, Clone, Copy)]
pub struct Weather {
    pub data: WeatherData,
    pub rain_level: f32,
    pub o_rain_level: f32,
    pub thunder_level: f32,
    pub o_thunder_level: f32,
    can_have_weather: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Weather {
    /// A level with no weather state yet.
    ///
    /// `can_have_weather` is `Level.canHaveWeather()`: `hasSkyLight() &&
    /// !hasCeiling() && dimension() != END`. True for the overworld, which is
    /// the only dimension this server serves.
    #[must_use]
    pub const fn new(can_have_weather: bool) -> Self {
        Self {
            data: WeatherData {
                clear_weather_time: 0,
                rain_time: 0,
                thunder_time: 0,
                raining: false,
                thundering: false,
            },
            rain_level: 0.0,
            o_rain_level: 0.0,
            thunder_level: 0.0,
            o_thunder_level: 0.0,
            can_have_weather,
        }
    }

    /// `ServerLevel.prepareWeather(WeatherData)` — snap the levels to a
    /// loaded save rather than ramping into it.
    pub const fn prepare(&mut self) {
        if self.data.raining {
            self.rain_level = 1.0;
            if self.data.thundering {
                self.thunder_level = 1.0;
            }
        }
    }

    /// `Level.getRainLevel(float)` — `Mth.lerp(p, oRainLevel, rainLevel)`.
    #[must_use]
    pub fn rain_level_at(&self, partial: f32) -> f32 {
        lerp(partial, self.o_rain_level, self.rain_level)
    }

    /// `Level.getThunderLevel(float)` — `Mth.lerp(p, oThunderLevel,
    /// thunderLevel) * getRainLevel(p)`. The multiply is the part that
    /// surprises: thunder is reported scaled by rain.
    #[must_use]
    pub fn thunder_level_at(&self, partial: f32) -> f32 {
        lerp(partial, self.o_thunder_level, self.thunder_level) * self.rain_level_at(partial)
    }

    /// `Level.isRaining()` — `canHaveWeather() && getRainLevel(1.0f) > 0.2`,
    /// with the widening to `double` the `f2d`/`dcmpl` pair performs.
    #[must_use]
    pub fn is_raining(&self) -> bool {
        self.can_have_weather && f64::from(self.rain_level_at(1.0)) > RAINING_ABOVE
    }

    /// `Level.isThundering()` — `canHaveWeather() && getThunderLevel(1.0f) >
    /// 0.9`.
    #[must_use]
    pub fn is_thundering(&self) -> bool {
        self.can_have_weather && f64::from(self.thunder_level_at(1.0)) > THUNDERING_ABOVE
    }

    /// `MinecraftServer.setWeatherParameters(int, int, boolean, boolean)`,
    /// entire — five setters, no draws, no packets.
    ///
    /// Note that the second argument is written to **both** `rainTime` and
    /// `thunderTime` (offsets 12-23). `/weather` is the only caller and it
    /// passes `0` for one of the two on every arm, so the aliasing is never
    /// visible through the command; it is transcribed because it is what the
    /// method does.
    pub const fn set_weather_parameters(
        &mut self,
        clear_time: i32,
        weather_time: i32,
        raining: bool,
        thundering: bool,
    ) {
        self.data.clear_weather_time = clear_time;
        self.data.rain_time = weather_time;
        self.data.thunder_time = weather_time;
        self.data.raining = raining;
        self.data.thundering = thundering;
    }

    /// `ServerLevel.resetWeatherCycle()` — the sleep path. `rainTime = 0`,
    /// `raining = false`, `thunderTime = 0`, `thundering = false`, in that
    /// order, and **`clearWeatherTime` is untouched**.
    pub const fn reset_weather_cycle(&mut self) {
        self.data.rain_time = 0;
        self.data.raining = false;
        self.data.thunder_time = 0;
        self.data.thundering = false;
    }

    /// `ServerLevel.advanceWeatherCycle()`, entire.
    ///
    /// `advance_weather` is `GameRules.ADVANCE_WEATHER`. `rng` is
    /// `level.random`, a `LegacyRandomSource` seeded at construction from
    /// `RandomSupport.generateUniqueSeed()` — **not reproducible across
    /// boots**, which is why the golden in this module's tests can only
    /// record internal consistency.
    ///
    /// Returns the packets to send, in vanilla's own send order.
    pub fn advance_weather_cycle(
        &mut self,
        advance_weather: bool,
        rng: &mut impl RandomSource,
    ) -> Vec<Send> {
        // Offsets 0-4. Taken before anything moves, and it is the *level*
        // predicate rather than the saved flag.
        let was_raining = self.is_raining();

        // Offset 9: `if (!canHaveWeather()) goto 372` — past the timers AND
        // past both ramps, onto the comparisons. Nothing has changed, so the
        // three guards below are all false and the method sends nothing.
        if self.can_have_weather {
            if advance_weather {
                self.advance_timers(rng);
            }
            // Offsets 262-371. Outside the game rule, inside `canHaveWeather`.
            // `step` is `LEVEL_STEP` unless T1's `weather-step` sabotage arm
            // is armed; without the `t1-sabotage` feature `fires` is `false`.
            let step = if rustmc_data::sabotage::fires(rustmc_data::sabotage::Arm::WeatherStep) {
                2.0 * LEVEL_STEP
            } else {
                LEVEL_STEP
            };
            self.o_thunder_level = self.thunder_level;
            self.thunder_level += if self.data.thundering { step } else { -step };
            self.thunder_level = clamp01(self.thunder_level);
            self.o_rain_level = self.rain_level;
            self.rain_level += if self.data.raining { step } else { -step };
            self.rain_level = clamp01(self.rain_level);
        }

        let mut out = Vec::new();
        // Offset 372. `fcmpl` + `ifeq` — a plain float inequality.
        if self.o_rain_level != self.rain_level {
            out.push(Send {
                event: Event::RainLevelChange,
                param: self.rain_level,
                audience: Audience::ThisDimension,
            });
        }
        // Offset 412.
        if self.o_thunder_level != self.thunder_level {
            out.push(Send {
                event: Event::ThunderLevelChange,
                param: self.thunder_level,
                audience: Audience::ThisDimension,
            });
        }
        // Offset 452. `if_icmpeq` on the two booleans.
        if was_raining != self.is_raining() {
            out.push(Send {
                // Offsets 460-506: the sense is `wasRaining ? STOP : START`,
                // which reads backwards until you remember it is the *old*
                // value being tested.
                event: if was_raining {
                    Event::StopRaining
                } else {
                    Event::StartRaining
                },
                param: 0.0,
                audience: Audience::EveryPlayer,
            });
            // Offsets 509 and 533. Both unconditional inside this arm, so
            // the rain level goes out twice on a transition tick.
            out.push(Send {
                event: Event::RainLevelChange,
                param: self.rain_level,
                audience: Audience::EveryPlayer,
            });
            out.push(Send {
                event: Event::ThunderLevelChange,
                param: self.thunder_level,
                audience: Audience::EveryPlayer,
            });
        }
        out
    }

    /// Offsets 36-261: the five locals, the three arms and the write-back.
    ///
    /// Split out of [`Self::advance_weather_cycle`] only so the arm structure
    /// is readable; it is inside the `ADVANCE_WEATHER` branch and nothing
    /// else calls it.
    fn advance_timers(&mut self, rng: &mut impl RandomSource) {
        // 36-63: five locals, read once.
        let mut clear_weather_time = self.data.clear_weather_time;
        let mut thunder_time = self.data.thunder_time;
        let mut rain_time = self.data.rain_time;
        let mut thundering = self.data.thundering;
        let mut raining = self.data.raining;

        if clear_weather_time > 0 {
            // 65-102. No draw, and a `goto` past both arms below.
            clear_weather_time -= 1;
            thunder_time = i32::from(!thundering);
            rain_time = i32::from(!raining);
            thundering = false;
            raining = false;
        } else {
            // 105-169: thunder first.
            if thunder_time > 0 {
                thunder_time -= 1;
                if thunder_time == 0 {
                    thundering = !thundering;
                }
            } else if thundering {
                thunder_time = THUNDER_DURATION.sample(rng);
            } else {
                thunder_time = THUNDER_DELAY.sample(rng);
            }
            // 169-233: rain second, same shape.
            if rain_time > 0 {
                rain_time -= 1;
                if rain_time == 0 {
                    raining = !raining;
                }
            } else if raining {
                rain_time = RAIN_DURATION.sample(rng);
            } else {
                rain_time = RAIN_DELAY.sample(rng);
            }
        }

        // 233-261, in the bytecode's own order.
        self.data.thunder_time = thunder_time;
        self.data.rain_time = rain_time;
        self.data.clear_weather_time = clear_weather_time;
        self.data.thundering = thundering;
        self.data.raining = raining;
    }

    /// `PlayerList.sendLevelInfo`'s weather arm, offsets 55-124.
    ///
    /// Empty when it is not raining. The caller sends
    /// `LEVEL_CHUNKS_LOAD_START` after these, which
    /// `crate::world_send::enter_world` already does.
    #[must_use]
    pub fn join_events(&self) -> Vec<(Event, f32)> {
        if !self.is_raining() {
            return Vec::new();
        }
        vec![
            (Event::StartRaining, 0.0),
            (Event::RainLevelChange, self.rain_level_at(1.0)),
            (Event::ThunderLevelChange, self.thunder_level_at(1.0)),
        ]
    }
}

/// `TimeArgument`'s unit table, from its `<clinit>` offsets 73-115:
/// `d` 24000, `s` 20, `t` 1, and the empty suffix 1. A unit not in the table
/// resolves to `getOrDefault(..., 0)` and is refused as
/// `argument.time.invalid_unit`.
const TIME_UNITS: &[(&str, i32)] = &[("d", 24_000), ("s", 20), ("t", 1), ("", 1)];

/// `TimeArgument.parse`, offsets 0-79.
///
/// `readFloat()` then `readUnquotedString()` for the suffix, then
/// `Math.round(value * unit)` — a **float** multiply rounded, not an integer
/// one, so `0.5s` is 10 ticks and `1.5t` is 2. `minimum` is the argument to
/// `TimeArgument.time(int)`, which `WeatherCommand` calls with `1` on all
/// three of its `duration` arguments; below it vanilla throws
/// `argument.time.tick_count_too_low`.
///
/// # Errors
/// On a value that does not parse as a float, an unrecognised unit, or a
/// result below `minimum`.
pub fn parse_time(text: &str, minimum: i32) -> Result<i32, String> {
    // `StringReader.readFloat` takes the longest prefix of `0-9`, `.` and
    // `-`; whatever follows is the unit. Splitting on that character class
    // is the same partition without a reader.
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: f32 = number
        .parse()
        .map_err(|_| format!("not a number: {number:?}"))?;
    let Some(&(_, factor)) = TIME_UNITS.iter().find(|(u, _)| *u == unit) else {
        return Err(format!("argument.time.invalid_unit: {unit:?}"));
    };
    // `Math.round(float)` rounds ties toward POSITIVE infinity — its
    // specification is `(int)Math.floor(a + 0.5f)` — where Rust's
    // `f32::round` rounds ties away from zero. They differ on every negative
    // half-integer: Java's `Math.round(-2.5f)` is -2 and `(-2.5f32).round()`
    // is -3. Nothing in this command can observe that (a negative result is
    // below `minimum` and refused either way, whichever of the two it is),
    // and it is written the jar's way regardless, because "unobservable
    // here" is a statement about today's callers.
    //
    // The one place this is NOT exact is the JDK-7 correction for arguments
    // within half an ulp below a half-integer, where `a + 0.5f` rounds up
    // and the intrinsic does not. `TimeArgument` reads its value with
    // `StringReader.readFloat`, so reaching it needs a literal like
    // `0.49999997`, and the result would be one tick either way.
    #[allow(clippy::cast_possible_truncation)]
    let ticks = (value * factor as f32 + 0.5).floor() as i32;
    if ticks < minimum {
        return Err(format!(
            "argument.time.tick_count_too_low: {ticks} < {minimum}"
        ));
    }
    Ok(ticks)
}

/// `WeatherCommand`'s three arms and their success messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherArm {
    /// `commands.weather.set.clear`
    Clear,
    /// `commands.weather.set.rain`
    Rain,
    /// `commands.weather.set.thunder`
    Thunder,
}

impl WeatherArm {
    /// The translation key `sendSuccess` is given, from each arm's
    /// `lambda$set*$0`.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Clear => "commands.weather.set.clear",
            Self::Rain => "commands.weather.set.rain",
            Self::Thunder => "commands.weather.set.thunder",
        }
    }
}

impl Weather {
    /// `WeatherCommand.setClear` / `.setRain` / `.setThunder`, whose bodies
    /// differ only in which `IntProvider` supplies a missing duration and
    /// which of the four `setWeatherParameters` arguments are set.
    ///
    /// ```text
    ///   setClear   (getDuration(t, RAIN_DELAY),        0, false, false)
    ///   setRain    (0, getDuration(t, RAIN_DURATION),    true,  false)
    ///   setThunder (0, getDuration(t, THUNDER_DURATION), true,  true )
    /// ```
    ///
    /// `getDuration(source, ticks, provider)` is `ticks == -1 ?
    /// provider.sample(source.getLevel().getRandom()) : ticks`, and `-1` is
    /// `WeatherCommand.DEFAULT_TIME` — the value the no-argument form
    /// passes. **So `/weather rain` with no duration draws one int and
    /// `/weather rain 100` draws none**, which is the only place in this
    /// subsystem where the presence of an argument changes the generator's
    /// position.
    ///
    /// Note `setClear` samples `RAIN_DELAY`, not a clear-specific provider:
    /// there is none.
    pub fn weather_command(
        &mut self,
        arm: WeatherArm,
        duration: Option<i32>,
        rng: &mut impl RandomSource,
    ) {
        // `DEFAULT_TIME = -1` reaches `getDuration` as a sentinel; `None`
        // here is that sentinel.
        let sample = |p: UniformInt, rng: &mut _| duration.unwrap_or_else(|| p.sample(rng));
        match arm {
            WeatherArm::Clear => {
                let t = sample(RAIN_DELAY, rng);
                self.set_weather_parameters(t, 0, false, false);
            }
            WeatherArm::Rain => {
                let t = sample(RAIN_DURATION, rng);
                self.set_weather_parameters(0, t, true, false);
            }
            WeatherArm::Thunder => {
                let t = sample(THUNDER_DURATION, rng);
                self.set_weather_parameters(0, t, true, true);
            }
        }
    }
}

/// `Mth.lerp(float, float, float)`, offsets 0-7 exactly:
/// `fload_1, fload_0, fload_2, fload_1, fsub, fmul, fadd` — that is
/// `a + (delta * (b - a))`, with `delta` first in the argument list.
///
/// **Three separate `f32` operations, and `mul_add` is not one of them.**
/// This function was written as `b.mul_add(delta, a.mul_add(-delta, a))`
/// first — clippy's `suboptimal_flops` asks for exactly that — and it is
/// twice wrong: `a*(1-delta) + b*delta` is a different expression from
/// `a + delta*(b - a)` in floating point, and `mul_add` is a **fused**
/// multiply-add that rounds once where the JVM's `fmul`/`fadd` round twice.
/// Vanilla emits no FMA here, so neither does this.
#[allow(clippy::suboptimal_flops)]
fn lerp(delta: f32, a: f32, b: f32) -> f32 {
    a + (delta * (b - a))
}

/// `Mth.clamp(float, float, float)` with `min = 0`, `max = 1`.
///
/// Vanilla's body is `value < min ? min : Math.min(value, max)`, offsets
/// 0-13. Three Rust spellings look right and two of them are not:
///
/// * `f32::clamp` **panics** on `min > max` and, more to the point, its
///   NaN behaviour is unspecified-by-contract rather than vanilla's;
/// * `value.min(1.0)` returns the **non-NaN** operand, so it yields `1.0`
///   for a NaN input. `java.lang.Math.min(float, float)` returns the NaN.
///   Writing `if value < 0.0 { 0.0 } else { value.min(1.0) }` therefore
///   silently converts a NaN level into a full storm;
/// * the third arm below is the one that agrees with the jar on all four
///   awkward inputs: NaN stays NaN (both comparisons are false, and
///   `NaN < min` is false in Java too, so `Math.min(NaN, max)` is reached
///   and returns NaN); `-0.0` stays `-0.0`; `1.0` stays `1.0`; `-1.0`
///   becomes `+0.0`.
///
/// Nothing in this module can produce a NaN — a level clamped into `[0, 1]`
/// and stepped by a finite constant stays finite — so this is a fidelity
/// point with no reachable consequence. It is written out because the
/// paragraph above it used to claim the NaN shape was transcribed while the
/// code returned `1.0`, which is this project's own named defect: a comment
/// describing behaviour no line implements.
// `manual_clamp` asks for `value.clamp(0.0, 1.0)`, which is the first of the
// three wrong spellings above.
#[allow(clippy::manual_clamp)]
fn clamp01(value: f32) -> f32 {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

/// `RandomSupport.SEED_UNIQUIFIER`'s initial value — `ldc2_w // long
/// 8682522807148012l` in that class's `<clinit>`.
const SEED_UNIQUIFIER_INIT: i64 = 8_682_522_807_148_012;

/// `RandomSupport.lambda$generateUniqueSeed$0` — `ldc2_w // long
/// 1181783497276652981l`, the multiplier the uniquifier is advanced by.
const SEED_UNIQUIFIER_MULTIPLIER: i64 = 1_181_783_497_276_652_981;

/// `RandomSupport.SEED_UNIQUIFIER`.
static SEED_UNIQUIFIER: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(SEED_UNIQUIFIER_INIT);

/// `RandomSupport.generateUniqueSeed()`, offsets 0-15:
/// `SEED_UNIQUIFIER.updateAndGet(x -> x * 1181783497276652981L) ^
/// System.nanoTime()`.
///
/// The multiply-and-xor is exact. **`System.nanoTime()` is substituted** by
/// the wall clock's nanosecond part: Java's is a monotonic counter with an
/// arbitrary origin, Rust exposes no equivalent free function, and nothing
/// downstream can tell them apart because the whole purpose of the term is
/// to be unpredictable. That substitution is recorded here rather than
/// hidden because the multiply-and-xor half *is* exact and a reader is
/// entitled to know which half.
fn generate_unique_seed() -> i64 {
    let uniquifier = SEED_UNIQUIFIER
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |x| Some(x.wrapping_mul(SEED_UNIQUIFIER_MULTIPLIER)),
        )
        .unwrap_or(SEED_UNIQUIFIER_INIT)
        .wrapping_mul(SEED_UNIQUIFIER_MULTIPLIER);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    uniquifier ^ nanos
}

/// `Level.random` — `RandomSource.create()`, a `LegacyRandomSource` seeded
/// once at level construction from [`generate_unique_seed`].
///
/// One generator, shared by the tick and by `/weather`, because vanilla has
/// one: `WeatherCommand.getDuration` samples
/// `source.getLevel().getRandom()`, which is the same field
/// `advanceWeatherCycle` draws its timers from. A second generator here
/// would put the two streams out of step with each other in a way no test
/// of either alone could see.
static LEVEL_RANDOM: std::sync::OnceLock<
    std::sync::Mutex<rustmc_worldgen::rng::legacy::LegacyRandomSource>,
> = std::sync::OnceLock::new();

/// [`LEVEL_RANDOM`].
#[must_use]
pub fn level_random() -> &'static std::sync::Mutex<rustmc_worldgen::rng::legacy::LegacyRandomSource>
{
    LEVEL_RANDOM.get_or_init(|| {
        std::sync::Mutex::new(rustmc_worldgen::rng::legacy::LegacyRandomSource::new(
            generate_unique_seed(),
        ))
    })
}

/// One console `weather` line, dispatched against the process's own weather
/// and generator.
///
/// The grammar is `WeatherCommand.register`'s, and only that: a literal from
/// {`clear`, `rain`, `thunder`} and an optional `duration`. Vanilla gates it
/// on `Commands.LEVEL_GAMEMASTERS` (permission 2); a console line has no
/// permission to check, exactly as `crate::console`'s header records for
/// `gamerule`.
///
/// # Errors
/// On an unknown or missing sub-command, a duration that
/// [`parse_time`] refuses, or a trailing argument the grammar has no node
/// for.
pub fn dispatch_console(args: &mut std::str::SplitWhitespace<'_>) -> Result<String, String> {
    let mut rng = level_random().lock().map_err(|e| e.to_string())?;
    let mut weather = shared().lock().map_err(|e| e.to_string())?;
    dispatch(args, &mut weather, &mut *rng)
}

/// [`dispatch_console`] against a caller-supplied level, so a test can drive
/// the grammar without touching the process-wide state two other threads
/// read.
///
/// # Errors
/// As [`dispatch_console`].
pub fn dispatch(
    args: &mut std::str::SplitWhitespace<'_>,
    weather: &mut Weather,
    rng: &mut impl RandomSource,
) -> Result<String, String> {
    let arm = match args.next() {
        Some("clear") => WeatherArm::Clear,
        Some("rain") => WeatherArm::Rain,
        Some("thunder") => WeatherArm::Thunder,
        Some(other) => return Err(format!("weather: no such sub-command: {other:?}")),
        None => return Err("weather: expected clear, rain or thunder".into()),
    };
    // `TimeArgument.time(1)` on all three `duration` arguments.
    let duration = match args.next() {
        Some(text) => Some(parse_time(text, 1).map_err(|e| format!("weather: {e}"))?),
        None => None,
    };
    if let Some(extra) = args.next() {
        return Err(format!("weather: unexpected argument {extra:?}"));
    }
    weather.weather_command(arm, duration, rng);
    Ok(arm.message().to_string())
}

/// The one weather state this process has.
///
/// A `Mutex` rather than atomics: `advance_weather_cycle` reads and writes
/// six fields and the packets it returns describe that whole transition, so
/// a torn read would produce a wire event describing a state no tick was in.
/// Locked for the duration of one tick's weather block, which is the same
/// scope `crate::broadcast::tick_lock` covers for the rest of the tick.
static SHARED: std::sync::OnceLock<std::sync::Mutex<Weather>> = std::sync::OnceLock::new();

/// The process-wide weather, matching `MinecraftServer.getWeatherData()`
/// being one object for the whole server.
#[must_use]
pub fn shared() -> &'static std::sync::Mutex<Weather> {
    SHARED.get_or_init(|| std::sync::Mutex::new(Weather::new(true)))
}

/// Whether `GameRules.ADVANCE_WEATHER` is on, read the way
/// `advanceWeatherCycle` reads it.
///
/// The 26.2 id is `advance_weather`; the pre-26.2 spelling `doWeatherCycle`
/// is not a rename this server accepts, for the reason
/// `crate::gamerules`'s header gives.
#[must_use]
pub fn advance_weather(rules: &GameRules) -> bool {
    match rules.get("advance_weather") {
        Some(crate::gamerules::GameRuleValue::Bool(b)) => b,
        // A store with no entry, or one holding an int: neither is reachable
        // through `GameRules::shared`, and vanilla's default is true.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;

    /// A generator whose draws are dictated, so a test can put a timer
    /// exactly where it wants it without searching for a seed. `nextInt`
    /// returns `script[i] % bound` and asserts the bound it was called with,
    /// which is how the four `UniformInt` bounds below are checked rather
    /// than assumed.
    struct Scripted {
        bounds: Vec<i32>,
        values: Vec<i32>,
        at: usize,
    }

    impl Scripted {
        fn new(values: &[i32]) -> Self {
            Self {
                bounds: Vec::new(),
                values: values.to_vec(),
                at: 0,
            }
        }
    }

    impl RandomSource for Scripted {
        fn set_seed(&mut self, _seed: i64) {}
        fn next_i32(&mut self) -> i32 {
            self.next_i32_bound(i32::MAX)
        }
        fn next_i32_bound(&mut self, bound: i32) -> i32 {
            self.bounds.push(bound);
            let v = self.values.get(self.at).copied().unwrap_or(0);
            self.at += 1;
            v
        }
        fn next_i64(&mut self) -> i64 {
            i64::from(self.next_i32())
        }
        fn next_bool(&mut self) -> bool {
            self.next_i32_bound(2) != 0
        }
        fn next_f32(&mut self) -> f32 {
            0.0
        }
        fn next_f64(&mut self) -> f64 {
            0.0
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    // ---------------------------------------------------------------- bounds

    /// The four `UniformInt.of` pairs and the single-draw shape of
    /// `Mth.randomBetweenInclusive`. The bound asserted is `max - min + 1`,
    /// which is the arithmetic at offsets 1-5 of that method.
    #[test]
    fn the_four_providers_draw_one_int_with_the_jars_bounds() {
        for (p, want_bound, want_min) in [
            (RAIN_DELAY, 168_001, 12_000),
            (RAIN_DURATION, 12_001, 12_000),
            (THUNDER_DELAY, 168_001, 12_000),
            (THUNDER_DURATION, 12_001, 3_600),
        ] {
            let mut rng = Scripted::new(&[0]);
            let v = p.sample(&mut rng);
            assert_eq!(rng.bounds, vec![want_bound], "one draw, bound max-min+1");
            assert_eq!(v, want_min, "a zero draw lands on the inclusive minimum");
        }
    }

    /// The positive control for the bound assertion above: a provider whose
    /// span is wrong is caught. Without this the previous test could be read
    /// as checking `Scripted` rather than the constants.
    #[test]
    fn a_wrong_span_would_change_the_bound() {
        let mut rng = Scripted::new(&[0]);
        UniformInt { min: 0, max: 0 }.sample(&mut rng);
        assert_eq!(rng.bounds, vec![1]);
        assert_ne!(rng.bounds, vec![168_001]);
    }

    // --------------------------------------------------------------- timers

    fn clearing(ticks: i32) -> Weather {
        let mut w = Weather::new(true);
        w.data.clear_weather_time = ticks;
        w
    }

    /// The clear arm: decrement, set the two timers to `!flag`, clear both
    /// flags, and **draw nothing**. The `0`/`1` values are the bytecode's,
    /// not a fresh sample.
    #[test]
    fn clear_weather_time_wins_and_costs_no_draw() {
        let mut w = clearing(5);
        w.data.raining = true;
        w.data.thundering = false;
        w.data.rain_time = 900;
        w.data.thunder_time = 900;
        let mut rng = Scripted::new(&[7, 7]);
        w.advance_weather_cycle(true, &mut rng);
        assert!(rng.bounds.is_empty(), "the clear arm samples nothing");
        assert_eq!(w.data.clear_weather_time, 4);
        // `raining` was true, so `rainTime = raining ? 0 : 1` is 0.
        assert_eq!(w.data.rain_time, 0);
        // `thundering` was false, so `thunderTime = thundering ? 0 : 1` is 1.
        assert_eq!(w.data.thunder_time, 1);
        assert!(!w.data.raining && !w.data.thundering);
    }

    /// And the arm below it does run once clearing is over — the positive
    /// control for the test above, so "no draw" cannot be read as "this
    /// generator is never used".
    #[test]
    fn with_clear_weather_time_at_zero_both_timers_sample() {
        let mut w = Weather::new(true);
        let mut rng = Scripted::new(&[0, 0]);
        w.advance_weather_cycle(true, &mut rng);
        // Thunder is consulted first, so its bound is the first entry.
        assert_eq!(rng.bounds, vec![168_001, 168_001]);
        assert_eq!(w.data.thunder_time, 12_000);
        assert_eq!(w.data.rain_time, 12_000);
    }

    /// Draw order: on a tick where both timers are exhausted and the flags
    /// differ, the two bounds are distinguishable and thunder's comes first.
    #[test]
    fn thunder_is_sampled_before_rain() {
        let mut w = Weather::new(true);
        w.data.thundering = true; // THUNDER_DURATION, bound 12001
        w.data.raining = false; // RAIN_DELAY,       bound 168001
        let mut rng = Scripted::new(&[0, 0]);
        w.advance_weather_cycle(true, &mut rng);
        assert_eq!(
            rng.bounds,
            vec![12_001, 168_001],
            "THUNDER_DURATION then RAIN_DELAY"
        );
    }

    /// A timer reaching zero flips its flag, and does so on the tick the
    /// decrement lands rather than the tick after.
    #[test]
    fn a_timer_hitting_zero_toggles_its_flag() {
        let mut w = Weather::new(true);
        w.data.rain_time = 1;
        w.data.thunder_time = 500;
        let mut rng = Scripted::new(&[]);
        w.advance_weather_cycle(true, &mut rng);
        assert_eq!(w.data.rain_time, 0);
        assert!(w.data.raining, "false -> true");
        assert!(rng.bounds.is_empty(), "a live timer draws nothing");

        // And back again from the other side.
        w.data.rain_time = 1;
        w.advance_weather_cycle(true, &mut rng);
        assert!(!w.data.raining, "true -> false");
    }

    /// `advance_weather false` freezes the timers and leaves the ramps
    /// running. This is the property `/weather` depends on under the corpus
    /// harness, which sets that rule false for every scenario.
    #[test]
    fn the_game_rule_gates_the_timers_and_not_the_levels() {
        let mut w = Weather::new(true);
        w.data.raining = true;
        w.data.rain_time = 42;
        let mut rng = Scripted::new(&[]);
        w.advance_weather_cycle(false, &mut rng);
        assert_eq!(w.data.rain_time, 42, "frozen");
        assert!((w.rain_level - LEVEL_STEP).abs() < f32::EPSILON, "ramping");
        assert!(rng.bounds.is_empty());

        // Positive control: with the rule on, the same state moves the timer.
        let mut on = Weather::new(true);
        on.data.raining = true;
        on.data.rain_time = 42;
        on.advance_weather_cycle(true, &mut rng);
        assert_eq!(on.data.rain_time, 41);
    }

    // --------------------------------------------------------------- levels

    /// The ramp: +0.01 while the saved flag says raining, -0.01 otherwise,
    /// clamped to [0, 1], with `o` holding the previous value.
    #[test]
    fn the_levels_step_by_one_hundredth_and_clamp() {
        let mut w = Weather::new(true);
        w.data.raining = true;
        let mut rng = Scripted::new(&[]);
        for _ in 0..200 {
            w.advance_weather_cycle(false, &mut rng);
        }
        assert!((w.rain_level - 1.0).abs() < f32::EPSILON, "clamped at 1");
        assert!((w.o_rain_level - 1.0).abs() < f32::EPSILON);
        w.data.raining = false;
        for _ in 0..200 {
            w.advance_weather_cycle(false, &mut rng);
        }
        assert!(w.rain_level.abs() < f32::EPSILON, "clamped at 0");
    }

    /// `getThunderLevel` multiplies by `getRainLevel`. Half rain and full
    /// thunder reports half.
    #[test]
    fn thunder_level_is_scaled_by_rain_level() {
        let mut w = Weather::new(true);
        w.rain_level = 0.5;
        w.o_rain_level = 0.5;
        w.thunder_level = 1.0;
        w.o_thunder_level = 1.0;
        assert!((w.thunder_level_at(1.0) - 0.5).abs() < f32::EPSILON);
        // The control: unscaled would be 1.0.
        assert!((w.thunder_level_at(1.0) - 1.0).abs() > 0.4);
    }

    /// `isRaining` crosses at strictly greater than 0.2 — and the tick it
    /// crosses on is decided by `f32` accumulation, not by decimal
    /// arithmetic.
    ///
    /// Exact decimal says the 21st tick: twenty steps of 0.01 is 0.20, which
    /// is not > 0.2. **Measured, it is the 20th.** `0.01f` is
    /// 0.00999999977648258, but the running `f32` sum of twenty of them
    /// rounds *up* to 0.20000002980232239, and the comparison happens in
    /// `double` after an `f2d` widening that preserves that overshoot
    /// exactly. So the predicate is true one tick earlier than the arithmetic
    /// a reader does in their head.
    ///
    /// This is not a rounding tolerance to be papered over: vanilla
    /// accumulates the same `float` in the same order and crosses on the
    /// same tick, and a port that carried the level as `f64` or as a tick
    /// count would cross on the 21st and be one tick wrong on every storm.
    #[test]
    fn is_raining_crosses_where_f32_accumulation_puts_it() {
        let mut w = Weather::new(true);
        w.data.raining = true;
        let mut rng = Scripted::new(&[]);
        let mut first = None;
        for tick in 1..=40 {
            w.advance_weather_cycle(false, &mut rng);
            if w.is_raining() && first.is_none() {
                first = Some(tick);
            }
        }
        assert_eq!(first, Some(20), "the f32 sum of 20 steps is > 0.2");
        // The control: the exact-decimal answer, so the line above cannot be
        // read as "whatever the code does".
        assert!(
            f64::from(0.01_f32) * 20.0 <= RAINING_ABOVE,
            "in double, twenty steps do NOT cross"
        );
    }

    /// A dimension with no weather runs neither the timers nor the ramps and
    /// sends nothing, however long it is ticked.
    #[test]
    fn a_dimension_without_weather_is_inert() {
        let mut w = Weather::new(false);
        w.data.raining = true;
        let mut rng = Scripted::new(&[0, 0]);
        for _ in 0..100 {
            assert!(w.advance_weather_cycle(true, &mut rng).is_empty());
        }
        assert!(w.rain_level.abs() < f32::EPSILON);
        assert!(rng.bounds.is_empty());
        assert!(!w.is_raining(), "canHaveWeather gates the predicate too");
    }

    // -------------------------------------------------------------- packets

    /// A plain ramp tick sends the two level changes, to this dimension
    /// only, and nothing else.
    #[test]
    fn a_ramping_tick_sends_two_dimension_scoped_level_changes() {
        let mut w = Weather::new(true);
        w.data.raining = true;
        w.data.thundering = true;
        let mut rng = Scripted::new(&[]);
        let sends = w.advance_weather_cycle(false, &mut rng);
        assert_eq!(sends.len(), 2);
        assert_eq!(sends[0].event, Event::RainLevelChange);
        assert_eq!(sends[1].event, Event::ThunderLevelChange);
        assert!(sends.iter().all(|s| s.audience == Audience::ThisDimension));
    }

    /// The transition tick: five packets, the rain level twice, and the
    /// last three to every player rather than to this dimension.
    #[test]
    fn the_transition_tick_sends_five_packets_and_repeats_the_rain_level() {
        let mut w = Weather::new(true);
        w.data.raining = true;
        let mut rng = Scripted::new(&[]);
        let mut transition = None;
        for _ in 0..40 {
            let sends = w.advance_weather_cycle(false, &mut rng);
            if sends.iter().any(|s| s.event == Event::StartRaining) {
                transition = Some(sends);
                break;
            }
        }
        let sends = transition.expect("a ramp up crosses 0.2");
        let kinds: Vec<Event> = sends.iter().map(|s| s.event).collect();
        // FOUR, not five, and the missing one is the guarded thunder send at
        // offset 412: nothing is thundering, so `thunderLevel` was already
        // clamped at 0 and `oThunderLevel != thunderLevel` is false. The
        // thunder send that *does* appear is the unconditional one at 533,
        // inside the transition arm — which is exactly the distinction
        // between a guarded send and an arm send, measured rather than
        // asserted.
        assert_eq!(
            kinds,
            vec![
                Event::RainLevelChange,    // 372, guarded, this dimension
                Event::StartRaining,       // 460, arm, every player
                Event::RainLevelChange,    // 509, arm, every player
                Event::ThunderLevelChange  // 533, arm, every player
            ],
            "offsets 372, 460, 509, 533"
        );
        assert_eq!(sends[0].audience, Audience::ThisDimension);
        assert!(sends[1..]
            .iter()
            .all(|s| s.audience == Audience::EveryPlayer));
        assert!(
            (sends[0].param - sends[2].param).abs() < f32::EPSILON,
            "the rain level goes out twice on a transition tick"
        );
        assert!(sends[1].param.abs() < f32::EPSILON, "the flag carries 0.0f");
        assert!(sends[3].param.abs() < f32::EPSILON, "thunder is still 0");
    }

    /// Ramping back down produces `STOP_RAINING`, which is the arm the
    /// `wasRaining ? STOP : START` sense decides.
    #[test]
    fn ramping_down_past_the_threshold_sends_stop_raining() {
        let mut w = Weather::new(true);
        w.data.raining = true;
        w.rain_level = 1.0;
        w.o_rain_level = 1.0;
        w.data.raining = false;
        let mut rng = Scripted::new(&[]);
        let mut seen = None;
        for tick in 1..=120 {
            let sends = w.advance_weather_cycle(false, &mut rng);
            if let Some(s) = sends
                .iter()
                .find(|s| s.event == Event::StopRaining || s.event == Event::StartRaining)
            {
                seen = Some((tick, s.event));
                break;
            }
        }
        // 81, not the 80 exact decimal gives, and for the mirror-image
        // reason `is_raining_crosses_where_f32_accumulation_puts_it` records:
        // the running `f32` subtraction from 1.0 is still fractionally above
        // 0.2 after eighty steps.
        assert_eq!(seen, Some((81, Event::StopRaining)));
    }

    /// The wire ids, straight off `ClientboundGameEventPacket.<clinit>`.
    #[test]
    fn the_game_event_ids_are_the_jars() {
        assert_eq!(Event::StartRaining as u8, 1);
        assert_eq!(Event::StopRaining as u8, 2);
        assert_eq!(Event::RainLevelChange as u8, 7);
        assert_eq!(Event::ThunderLevelChange as u8, 8);
    }

    // ----------------------------------------------------------------- join

    /// `sendLevelInfo` sends nothing about weather to a player joining a dry
    /// world, and three packets to one joining a wet one — with the thunder
    /// value already scaled by rain.
    #[test]
    fn a_joining_player_hears_about_rain_only_when_it_is_raining() {
        let mut w = Weather::new(true);
        assert!(w.join_events().is_empty(), "dry: no packets at all");

        w.rain_level = 0.5;
        w.o_rain_level = 0.5;
        w.thunder_level = 0.5;
        w.o_thunder_level = 0.5;
        let ev = w.join_events();
        assert_eq!(
            ev.iter().map(|e| e.0).collect::<Vec<_>>(),
            vec![
                Event::StartRaining,
                Event::RainLevelChange,
                Event::ThunderLevelChange
            ]
        );
        assert!(ev[0].1.abs() < f32::EPSILON);
        assert!((ev[1].1 - 0.5).abs() < f32::EPSILON);
        assert!((ev[2].1 - 0.25).abs() < f32::EPSILON, "0.5 * 0.5");
    }

    // ------------------------------------------------------------ set / reset

    /// `setWeatherParameters` writes `weather_time` into both timers.
    #[test]
    fn set_weather_parameters_writes_the_duration_into_both_timers() {
        let mut w = Weather::new(true);
        w.set_weather_parameters(11, 22, true, false);
        assert_eq!(w.data.clear_weather_time, 11);
        assert_eq!(w.data.rain_time, 22);
        assert_eq!(w.data.thunder_time, 22);
        assert!(w.data.raining && !w.data.thundering);
    }

    /// `resetWeatherCycle` leaves `clearWeatherTime` alone, which is the
    /// detail a from-memory port gets wrong.
    #[test]
    fn reset_weather_cycle_leaves_clear_weather_time_alone() {
        let mut w = clearing(1234);
        w.data.raining = true;
        w.data.thundering = true;
        w.data.rain_time = 9;
        w.reset_weather_cycle();
        assert_eq!(w.data.clear_weather_time, 1234);
        assert_eq!(w.data.rain_time, 0);
        assert!(!w.data.raining && !w.data.thundering);
    }

    /// `prepareWeather` snaps rather than ramps, and only sets thunder when
    /// it is also raining.
    #[test]
    fn prepare_snaps_the_levels_to_the_saved_flags() {
        let mut dry = Weather::new(true);
        dry.prepare();
        assert!(dry.rain_level.abs() < f32::EPSILON);

        let mut wet = Weather::new(true);
        wet.data.raining = true;
        wet.prepare();
        assert!((wet.rain_level - 1.0).abs() < f32::EPSILON);
        assert!(wet.thunder_level.abs() < f32::EPSILON, "not thundering");

        let mut storm = Weather::new(true);
        storm.data.raining = true;
        storm.data.thundering = true;
        storm.prepare();
        assert!((storm.thunder_level - 1.0).abs() < f32::EPSILON);

        // The nesting matters: thundering without raining sets neither.
        let mut odd = Weather::new(true);
        odd.data.thundering = true;
        odd.prepare();
        assert!(odd.thunder_level.abs() < f32::EPSILON);
    }

    /// `Mth.lerp` is `fsub, fmul, fadd` — three roundings, in that order.
    ///
    /// The two things this pins are the ones a rewrite gets wrong: the
    /// **argument order** (`delta` first, which reads backwards), and that
    /// the result at `delta = 1.0` is `a + (b - a)` rather than `b`. Those
    /// are the same number here and are not the same expression, which is
    /// why the assertion is written against the expression.
    #[test]
    fn lerp_is_the_jars_three_operations_in_the_jars_order() {
        let (a, b) = (0.25_f32, 0.75_f32);
        assert!((lerp(0.0, a, b) - a).abs() < f32::EPSILON, "delta first");
        assert!((lerp(1.0, a, b) - (a + (b - a))).abs() < f32::EPSILON);
        assert!((lerp(0.5, a, b) - 0.5).abs() < f32::EPSILON);
        // The control for the argument order: swapping the ends must move
        // the answer, so the test above cannot be satisfied by a symmetric
        // mistake.
        assert!((lerp(0.25, a, b) - lerp(0.25, b, a)).abs() > 0.2);
    }

    /// `Mth.clamp`'s four awkward inputs, against `java.lang.Math.min`'s
    /// rules rather than Rust's.
    #[test]
    fn clamp01_agrees_with_the_jar_on_nan_and_the_signed_zeroes() {
        assert!((clamp01(-1.0) - 0.0).abs() < f32::EPSILON);
        assert!((clamp01(2.0) - 1.0).abs() < f32::EPSILON);
        assert!((clamp01(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(clamp01(-0.0).is_sign_negative(), "Math.min keeps -0.0");
        // The one that separates this from `value.min(1.0)`, which would
        // return 1.0 here and turn a NaN level into a full storm.
        assert!(clamp01(f32::NAN).is_nan(), "Math.min(NaN, max) is NaN");
        assert!(!(f32::NAN.min(1.0)).is_nan(), "and Rust's min is not");
    }

    // ------------------------------------------------------------- command

    /// `TimeArgument`'s unit table and its float-round, from the four units
    /// in the jar's `<clinit>` plus the `Math.round` at offset 40.
    #[test]
    fn parse_time_reads_the_jars_units_and_rounds_a_float() {
        assert_eq!(parse_time("100", 1), Ok(100));
        assert_eq!(parse_time("100t", 1), Ok(100));
        assert_eq!(parse_time("5s", 1), Ok(100));
        assert_eq!(parse_time("1d", 1), Ok(24_000));
        // The multiply is in float and the result is rounded, not truncated.
        assert_eq!(parse_time("0.5s", 1), Ok(10));
        assert_eq!(parse_time("1.5t", 1), Ok(2));
        // Ties go toward positive infinity, not away from zero: `Math.round`
        // is `floor(a + 0.5f)`. 2.5 -> 3 agrees with both rules; the
        // distinguishing case is negative and is refused by the minimum,
        // which is why it is asserted as a refusal rather than a value.
        assert_eq!(parse_time("2.5t", 1), Ok(3));
        assert!(parse_time("-2.5t", 1).is_err());
        // Below `TimeArgument.time(1)`'s minimum.
        assert!(parse_time("0", 1).is_err(), "tick_count_too_low");
        assert!(parse_time("-5s", 1).is_err());
        // A unit not in the table.
        assert!(parse_time("5h", 1).is_err(), "invalid_unit");
        assert!(parse_time("h", 1).is_err(), "not a number");
    }

    /// The three command arms, each against the four arguments its
    /// `setWeatherParameters` call passes.
    #[test]
    fn the_three_weather_arms_set_what_the_jar_sets() {
        let mut rng = Scripted::new(&[]);

        let mut w = Weather::new(true);
        w.weather_command(WeatherArm::Clear, Some(300), &mut rng);
        assert_eq!(
            w.data,
            WeatherData {
                clear_weather_time: 300,
                ..WeatherData::default()
            }
        );

        let mut w = Weather::new(true);
        w.weather_command(WeatherArm::Rain, Some(300), &mut rng);
        assert_eq!(
            w.data,
            WeatherData {
                clear_weather_time: 0,
                rain_time: 300,
                thunder_time: 300,
                raining: true,
                thundering: false,
            }
        );

        let mut w = Weather::new(true);
        w.weather_command(WeatherArm::Thunder, Some(300), &mut rng);
        assert!(w.data.raining && w.data.thundering);
        assert_eq!(w.data.clear_weather_time, 0);

        assert!(rng.bounds.is_empty(), "an explicit duration draws nothing");
    }

    /// `getDuration`'s `DEFAULT_TIME == -1` arm: the no-argument form draws
    /// exactly one int, from that arm's own provider. The bounds
    /// distinguish all three, which is what makes this a check on the
    /// provider choice rather than on "something was drawn".
    #[test]
    fn a_weather_command_with_no_duration_samples_that_arms_provider() {
        for (arm, bound, want) in [
            (WeatherArm::Clear, 168_001, 12_000), // RAIN_DELAY
            (WeatherArm::Rain, 12_001, 12_000),   // RAIN_DURATION
            (WeatherArm::Thunder, 12_001, 3_600), // THUNDER_DURATION
        ] {
            let mut rng = Scripted::new(&[0]);
            let mut w = Weather::new(true);
            w.weather_command(arm, None, &mut rng);
            assert_eq!(rng.bounds, vec![bound], "{arm:?}");
            let got = if arm == WeatherArm::Clear {
                w.data.clear_weather_time
            } else {
                w.data.rain_time
            };
            assert_eq!(got, want, "{arm:?}");
        }
    }

    /// The success keys, which are the strings the arms' lambdas load.
    #[test]
    fn the_arms_carry_the_jars_translation_keys() {
        assert_eq!(WeatherArm::Clear.message(), "commands.weather.set.clear");
        assert_eq!(WeatherArm::Rain.message(), "commands.weather.set.rain");
        assert_eq!(
            WeatherArm::Thunder.message(),
            "commands.weather.set.thunder"
        );
    }

    /// End to end through the mechanism this command exists to reach:
    /// `/weather rain 300` then ticking under `advance_weather false` — the
    /// harness substrate — ramps the level and crosses the threshold. This
    /// is the property the corpus scenario would measure.
    #[test]
    fn weather_rain_then_ticks_reaches_a_start_raining_send() {
        let mut rng = Scripted::new(&[]);
        let mut w = Weather::new(true);
        w.weather_command(WeatherArm::Rain, Some(300), &mut rng);
        let mut at = None;
        for tick in 1..=60 {
            let sends = w.advance_weather_cycle(false, &mut rng);
            if sends.iter().any(|s| s.event == Event::StartRaining) {
                at = Some(tick);
                break;
            }
        }
        assert_eq!(at, Some(20), "one second of ramp, then the crossing");
        // The control: without the command, sixty ticks send nothing at all.
        let mut dry = Weather::new(true);
        for _ in 0..60 {
            assert!(dry.advance_weather_cycle(false, &mut rng).is_empty());
        }
    }

    /// `ClientboundGameEventPacket.write`'s body: one byte then a
    /// big-endian float, five bytes, no varint anywhere.
    #[test]
    fn a_send_encodes_to_a_byte_and_a_big_endian_float() {
        let b = Send {
            event: Event::RainLevelChange,
            param: 0.5,
            audience: Audience::ThisDimension,
        }
        .body();
        assert_eq!(b, vec![7, 0x3f, 0x00, 0x00, 0x00]);
        assert_eq!(b.len(), 5);
    }

    // ------------------------------------------------------- console grammar

    fn console(line: &str, w: &mut Weather, rng: &mut Scripted) -> Result<String, String> {
        let mut parts = line.split_whitespace();
        assert_eq!(parts.next(), Some("weather"));
        dispatch(&mut parts, w, rng)
    }

    /// The grammar `WeatherCommand.register` builds: three literals, each
    /// with an optional `duration` child and no other node.
    #[test]
    fn the_console_grammar_is_weather_commands_and_nothing_more() {
        let mut rng = Scripted::new(&[0, 0, 0]);
        let mut w = Weather::new(true);

        assert_eq!(
            console("weather rain", &mut w, &mut rng),
            Ok("commands.weather.set.rain".into())
        );
        assert!(w.data.raining && !w.data.thundering);
        assert_eq!(
            console("weather clear", &mut w, &mut rng),
            Ok("commands.weather.set.clear".into())
        );
        assert!(!w.data.raining);
        assert_eq!(
            console("weather thunder 30s", &mut w, &mut rng),
            Ok("commands.weather.set.thunder".into())
        );
        assert_eq!(w.data.thunder_time, 600, "30s is 600 ticks");

        // Every refusal the grammar has, and each must be an Err rather than
        // a silent no-op — the 2026-08-03 defect this console exists after.
        for bad in [
            "weather",
            "weather sunny",
            "weather rain 0",
            "weather rain 5h",
            "weather rain 100 200",
        ] {
            assert!(console(bad, &mut w, &mut rng).is_err(), "{bad:?}");
        }
    }

    /// `RandomSupport.generateUniqueSeed`'s multiply-and-xor: the uniquifier
    /// advances by the jar's multiplier on every call, so two calls cannot
    /// return the same value even inside one nanosecond-clock tick.
    #[test]
    fn the_seed_uniquifier_advances() {
        let a = generate_unique_seed();
        let b = generate_unique_seed();
        assert_ne!(a, b);
        assert_eq!(
            SEED_UNIQUIFIER_INIT.wrapping_mul(SEED_UNIQUIFIER_MULTIPLIER),
            3_447_679_086_515_839_964,
            "the jar's two constants, multiplied once"
        );
    }

    // --------------------------------------------------------------- golden

    /// A seeded run of the whole cycle, recorded so that a change to the arm
    /// structure, the draw order or the constants moves it.
    ///
    /// **This is an internal-consistency golden and nothing more, and the
    /// reason is a property of vanilla rather than of this harness.**
    /// `Level.random` is `RandomSource.create()` — a `LegacyRandomSource`
    /// seeded from `RandomSupport.generateUniqueSeed()` at level
    /// construction, which is a fresh `SecureRandom`-derived value on every
    /// boot. There is therefore no vanilla run of "seed 12345" for this
    /// sequence to be compared against, and none can be produced: the
    /// sequence below has never been observed anywhere but here. It cannot
    /// show the port is right. It can only show it has not changed.
    ///
    /// The evidence that the port *is* right is the per-arm tests above,
    /// each of which names the offsets it transcribes; this pins the way
    /// those arms compose over a hundred thousand ticks, which no per-arm
    /// test does.
    ///
    /// What is recorded: every tick at which a saved flag flips, plus the
    /// timer values written on that tick, for the first eight flips.
    #[test]
    fn a_seeded_cycle_reproduces_its_own_timer_sequence() {
        let mut rng = LegacyRandomSource::new(12_345);
        let mut w = Weather::new(true);
        let mut log: Vec<String> = Vec::new();
        let mut was = (false, false);
        for tick in 0..600_000_i64 {
            w.advance_weather_cycle(true, &mut rng);
            let now = (w.data.raining, w.data.thundering);
            if now != was {
                log.push(format!(
                    "{tick} raining={} thundering={} rain_time={} thunder_time={}",
                    now.0, now.1, w.data.rain_time, w.data.thunder_time
                ));
                was = now;
                if log.len() == 8 {
                    break;
                }
            }
        }
        assert_eq!(
            log, GOLDEN_FLIPS,
            "internal consistency only — see this test's doc"
        );
    }

    /// The recorded sequence. Regenerate deliberately, never to make a test
    /// pass: a change here means the arm structure moved, and the commit
    /// that moves it has to say which arm and why.
    const GOLDEN_FLIPS: &[&str] = &[
        "34520 raining=true thundering=false rain_time=0 thunder_time=107107",
        "55811 raining=false thundering=false rain_time=0 thunder_time=85816",
        "80917 raining=true thundering=false rain_time=0 thunder_time=60710",
        "93898 raining=false thundering=false rain_time=0 thunder_time=47729",
        "141627 raining=false thundering=true rain_time=57875 thunder_time=0",
        "148183 raining=false thundering=false rain_time=51319 thunder_time=0",
        "199502 raining=true thundering=false rain_time=0 thunder_time=57891",
        "212859 raining=false thundering=false rain_time=0 thunder_time=44534",
    ];
}
