//! worldedit-core — selection, set/copy/paste, schematics: the 20% of
//! WorldEdit that covers ~90% of real usage.
//! First-party plugin: depends on rustmc-api ONLY (enforced workspace rule).
//!
//! ```
//! use worldedit_core::WorldEditCore;
//! use rustmc_api::Plugin;
//!
//! assert_eq!(WorldEditCore.name(), "worldedit-core");
//! ```

use rustmc_api::{Plugin, Server};

pub struct WorldEditCore;

impl Plugin for WorldEditCore {
    fn name(&self) -> &'static str {
        "worldedit-core"
    }

    fn enable(&mut self, _server: &mut dyn Server) {
        // wave 4: //wand //pos1 //pos2 //set //copy //paste, .schem IO
    }
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// It is one of the nine lib/bin targets `ci/t0.sh` selects with `--lib
/// --bins` that reported `test result: ok. 0 passed; 0 failed; 0 ignored` on
/// every gate this project has ever run — declared covered by
/// `ci/checks/test-targets.toml`'s `t0-unit` bucket and incapable of failing.
///
/// The tests are deliberately not `assert_eq!(WorldEditCore.name(),
/// "worldedit-core")` on its own: a constant compared against a copy of itself is
/// the defect `docs/AGENT-BRIEF.md` opens with. What is checked is the part
/// of the contract that is written somewhere other than in the value —
/// [`Plugin::name`]'s documented shape, and this crate's own manifest against
/// the isolation rule in the header above.
#[cfg(test)]
mod tests {
    use super::WorldEditCore;
    use rustmc_api::{Event, EventOutcome, Plugin, Server};

    /// A minimal host, written against `rustmc_api` and nothing else — which
    /// is the point, and which did not compile until `rustmc-api` re-exported
    /// the types its own signatures name (2026-08-16).
    #[derive(Default)]
    struct Host {
        commands: Vec<String>,
        handlers: usize,
    }

    impl Server for Host {
        fn on_event(&mut self, _h: Box<dyn FnMut(&Event) -> EventOutcome + Send>) {
            self.handlers += 1;
        }
        fn dispatch_command(&mut self, command: &str) {
            self.commands.push(command.to_string());
        }
        fn send_message(&mut self, _player: rustmc_api::EntityId, _message: &str) {}
    }

    /// `Plugin::name` is documented as a "stable machine name (lowercase, no
    /// spaces) — config keys, data dirs". That is a contract about the
    /// *shape* of the string, so it is checkable without restating the value.
    #[test]
    fn the_plugin_name_satisfies_the_documented_machine_name_shape() {
        let name = WorldEditCore.name();
        assert!(!name.is_empty(), "a plugin with no name has no config key");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{name:?} is not a lowercase machine name"
        );
        assert!(
            !name.contains(' '),
            "{name:?} would not survive a directory name"
        );
    }

    /// The entry point has to be drivable through the API as a trait object,
    /// because that is how the server holds it. A `wave 4` stub registers
    /// nothing, and that is a real fact about today rather than a placeholder:
    /// a stub that quietly started dispatching commands on enable would be a
    /// behaviour change nothing else would notice.
    #[test]
    fn enable_drives_through_the_api_and_registers_nothing_yet() {
        let mut host = Host::default();
        let mut plugin: Box<dyn Plugin> = Box::new(WorldEditCore);
        plugin.enable(&mut host);
        plugin.disable();
        assert_eq!(
            host.handlers, 0,
            "wave 4 has not started; update this when it does"
        );
        assert!(host.commands.is_empty());
    }

    /// **The header's "enforced workspace rule", actually enforced.**
    ///
    /// It was enforced by nothing until 2026-08-16 and could not be obeyed
    /// either — see `rustmc-api`'s own note. The manifest is read through
    /// `include_str!`, so the bytes tested are the bytes cargo built this
    /// crate from; a test that read the file at runtime would be reading
    /// whatever the working directory happened to hold.
    #[test]
    fn this_plugin_depends_on_rustmc_api_and_nothing_else() {
        let manifest = include_str!("../Cargo.toml");
        assert_eq!(
            dependency_keys(manifest, "[dependencies]"),
            vec!["rustmc-api"],
            "a plugins/ crate may name no dependency but rustmc-api"
        );
        assert!(
            dependency_keys(manifest, "[dev-dependencies]").is_empty(),
            "the rule is about what this crate may see at all, so dev-dependencies \
             are not an exemption — a test helper crate would still be a second \
             surface a plugin author could reach for"
        );
    }

    /// The parser has to be able to see a violation, or the test above is a
    /// green that cannot be turned red.
    #[test]
    fn the_manifest_reader_sees_a_second_dependency() {
        let bad = "[package]\nname = \"x\"\n\n[dependencies]\nrustmc-api.workspace = true\nserde = \"1\"\n\n[lints]\n";
        assert_eq!(
            dependency_keys(bad, "[dependencies]"),
            vec!["rustmc-api", "serde"]
        );
        assert!(dependency_keys(bad, "[dev-dependencies]").is_empty());
    }

    /// The keys of one TOML table, in file order. Deliberately not a TOML
    /// crate: pulling one in would itself be a dependency this crate is not
    /// allowed to have.
    fn dependency_keys(manifest: &str, table: &str) -> Vec<String> {
        let mut keys = Vec::new();
        let mut inside = false;
        for line in manifest.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                inside = line == table;
                continue;
            }
            if !inside || line.is_empty() || line.starts_with('#') {
                continue;
            }
            let key = line
                .split(['=', '.'])
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
            if !key.is_empty() && !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }
}
