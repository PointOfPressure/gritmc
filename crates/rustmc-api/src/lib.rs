//! rustmc-api — the public plugin API. THE product surface.
//!
//! Semver 0.1.0 from this commit. Growth policy (CRATES.md): the API grows
//! ONLY by demand from the three first-party plugins during wave 4, then
//! freezes for launch. Every method a plugin didn't need is a method we
//! maintain forever for no one.
//!
//! Day-0 API citizens (decided, VISION.md): packet-level access (kills the
//! ProtocolLib workaround class), economy/placeholder interfaces (kills
//! Vault/PAPI), multi-instance worlds (core, kills Multiverse).
//!
//! ENFORCED: plugins/ crates depend on rustmc-api ONLY. Nothing here leaks
//! server internals; everything is trait-shaped so the server can evolve
//! underneath.

/// # THE `ONLY` ABOVE WAS NOT SATISFIABLE UNTIL 2026-08-16
///
/// The header says plugins depend on `rustmc-api` **only**, and each of the
/// three first-party plugins repeats it as "enforced workspace rule". Nothing
/// enforced it, and — found while giving `permissions`, `essentials-core` and
/// `worldedit-core` their first executable assertion — **nothing could**:
/// every type in this crate's public signatures came from somewhere else and
/// none of them were reachable through this crate. A plugin cannot implement
/// [`Server`] without naming [`EntityId`], cannot bind the fields of a
/// [`Event::BlockPlace`] without [`BlockPos`] and [`BlockStateId`], and so
/// could only satisfy the API by breaking the rule the API states.
///
/// So the rule held the way `ACCEPT_TELEPORTATION` held: a doc comment
/// describing a gate no line implemented, over a surface nobody had tried to
/// use from the outside. These three re-exports are what make it a rule a
/// plugin can obey, and `plugins/*/src/lib.rs` now assert their own manifests
/// against it.
///
/// They are re-exports and not new types on purpose: a mirrored `EntityId`
/// would make a plugin's entity a different type from the server's, which is
/// the mistake the workspace `Cargo.toml` records having refused for
/// `grit-plugin-api`.
pub use rustmc_data::{BlockPos, BlockStateId};
pub use rustmc_entity::EntityId;

/// Events plugins can observe. Extends by demand in wave 4; each variant's
/// fields are wire-observable facts, not internal state.
#[derive(Debug)]
#[non_exhaustive]
pub enum Event {
    PlayerJoin {
        player: EntityId,
        name: String,
    },
    PlayerQuit {
        player: EntityId,
    },
    PlayerChat {
        player: EntityId,
        message: String,
    },
    BlockPlace {
        player: EntityId,
        pos: BlockPos,
        state: BlockStateId,
    },
    BlockBreak {
        player: EntityId,
        pos: BlockPos,
        state: BlockStateId,
    },
}

/// Verdict from an event handler. Cancellable events honor `Cancel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOutcome {
    Allow,
    Cancel,
}

/// What a plugin gets to talk to. Handed in at load; valid for the plugin's
/// lifetime. All methods are safe to call from event handlers (they enqueue
/// through the region message model — regionization rule 2 — never reach
/// into another region's state directly).
pub trait Server {
    /// Register an event handler. Handlers run in registration order
    /// (deterministic; order is observable when two plugins contend).
    fn on_event(&mut self, handler: Box<dyn FnMut(&Event) -> EventOutcome + Send>);

    /// Run a command as the console.
    fn dispatch_command(&mut self, command: &str);

    /// Send a chat message to a player.
    fn send_message(&mut self, player: EntityId, message: &str);
}

/// FROZEN INTERFACE: the plugin entry point.
///
/// ```
/// use rustmc_api::{Plugin, Server};
///
/// struct Greeter;
/// impl Plugin for Greeter {
///     fn name(&self) -> &'static str { "greeter" }
///     fn enable(&mut self, _server: &mut dyn Server) {}
/// }
/// ```
pub trait Plugin: Send {
    /// Stable machine name (lowercase, no spaces) — config keys, data dirs.
    fn name(&self) -> &'static str;

    /// Called once when the server enables the plugin. Register events and
    /// commands here.
    fn enable(&mut self, server: &mut dyn Server);

    /// Called on shutdown/reload. Default: nothing to clean up.
    fn disable(&mut self) {}
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// `rustmc-api` is "THE product surface", semver 0.1.0, and its header says it
/// "freezes for launch". A frozen surface with no test is a promise, not a
/// property — so what is asserted here is the *freeze*, in the two directions
/// it can break.
///
/// # Why a list of names is a ratchet here and not a tautology
///
/// A test comparing a constant against a copy of itself is this project's
/// central defect. This is the other thing: the exhaustive `match` below
/// cannot compile if a variant is added, renamed or removed, and the name
/// list beside it cannot pass if the spelling changes. Neither says the
/// surface is *correct*. Both say that changing it costs a deliberate edit
/// here, which is what "frozen" means and is the same shape as
/// `min_binaries` in `ci/checks/t1-known-red.toml`.
#[cfg(test)]
mod tests {
    use super::{BlockPos, BlockStateId, EntityId, Event, EventOutcome, Plugin, Server};

    /// The rule in this crate's header, exercised from the outside: a type
    /// that only `rustmc-api` is allowed to name must be nameable *through*
    /// `rustmc-api`. Before the re-exports above this did not compile.
    #[test]
    fn every_type_in_the_public_signatures_is_reachable_from_this_crate() {
        let pos: BlockPos = BlockPos {
            x: -1,
            y: 64,
            z: 16,
        };
        let state: BlockStateId = BlockStateId(3);
        let who: EntityId = EntityId(7);
        assert_eq!(pos.x, -1);
        assert_eq!(state.0, 3);
        assert_eq!(who.0, 7);
        let e = Event::BlockPlace {
            player: who,
            pos,
            state,
        };
        assert!(matches!(e, Event::BlockPlace { .. }));
    }

    /// Adding a variant breaks this match at compile time; removing or
    /// renaming one breaks it too. `#[non_exhaustive]` does not apply inside
    /// the defining crate, which is exactly why the freeze can be pinned here
    /// and nowhere else.
    #[test]
    fn the_event_surface_is_frozen_at_its_declared_variants() {
        fn name(e: &Event) -> &'static str {
            match e {
                Event::PlayerJoin { .. } => "PlayerJoin",
                Event::PlayerQuit { .. } => "PlayerQuit",
                Event::PlayerChat { .. } => "PlayerChat",
                Event::BlockPlace { .. } => "BlockPlace",
                Event::BlockBreak { .. } => "BlockBreak",
            }
        }
        let all = [
            Event::PlayerJoin {
                player: EntityId(7),
                name: String::new(),
            },
            Event::PlayerQuit {
                player: EntityId(7),
            },
            Event::PlayerChat {
                player: EntityId(7),
                message: String::new(),
            },
            Event::BlockPlace {
                player: EntityId(7),
                pos: BlockPos {
                    x: -1,
                    y: 64,
                    z: 16,
                },
                state: BlockStateId(3),
            },
            Event::BlockBreak {
                player: EntityId(7),
                pos: BlockPos {
                    x: -1,
                    y: 64,
                    z: 16,
                },
                state: BlockStateId(3),
            },
        ];
        let names: Vec<&str> = all.iter().map(name).collect();
        assert_eq!(
            names,
            [
                "PlayerJoin",
                "PlayerQuit",
                "PlayerChat",
                "BlockPlace",
                "BlockBreak"
            ],
            "the day-0 event surface moved — update this list deliberately and say so"
        );
    }

    /// `Cancel` and `Allow` are distinguishable, and a handler that returns
    /// one is not returning the other. Trivial to state and the reason it is
    /// here is that nothing in this workspace had ever constructed either.
    #[test]
    fn the_two_outcomes_are_distinct() {
        assert_ne!(EventOutcome::Allow, EventOutcome::Cancel);
        assert_eq!(EventOutcome::Allow, EventOutcome::Allow);
    }

    /// The traits have to be usable as trait objects, because that is how the
    /// server holds a plugin (`Box<dyn Plugin>`) and how a plugin is handed
    /// the server (`&mut dyn Server`). A signature change that quietly costs
    /// object safety would break every plugin and nothing here.
    #[test]
    fn plugin_and_server_are_object_safe_and_a_plugin_can_be_driven() {
        struct Recorder {
            said: Vec<String>,
        }
        impl Server for Recorder {
            fn on_event(&mut self, _h: Box<dyn FnMut(&Event) -> EventOutcome + Send>) {}
            fn dispatch_command(&mut self, command: &str) {
                self.said.push(format!("cmd:{command}"));
            }
            fn send_message(&mut self, _player: EntityId, message: &str) {
                self.said.push(format!("msg:{message}"));
            }
        }
        struct Greeter;
        impl Plugin for Greeter {
            fn name(&self) -> &'static str {
                "greeter"
            }
            fn enable(&mut self, server: &mut dyn Server) {
                server.dispatch_command("say hello");
                server.send_message(EntityId(7), "hi");
            }
        }

        let mut server = Recorder { said: Vec::new() };
        let mut plugin: Box<dyn Plugin> = Box::new(Greeter);
        plugin.enable(&mut server);
        plugin.disable();
        assert_eq!(plugin.name(), "greeter");
        assert_eq!(server.said, ["cmd:say hello", "msg:hi"]);
    }
}
