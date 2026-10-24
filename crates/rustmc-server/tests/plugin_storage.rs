//! `grit:storage.dir`, end to end, on the real server host.
//!
//! # Why this is a test binary of its own
//!
//! The thing under test is conditional on a **process-global `OnceLock`** —
//! `Tiles::persist_to`, set once from `main.rs` when the operator passes
//! `--world`. A test that sets it changes the answer for every other test in
//! the same process, and `cargo test` runs a module's tests on parallel
//! threads. `plugin_tick_lock.rs` is here for the same reason and its header
//! records what that flakiness cost: a mutation caught on one run and missed
//! on the next.
//!
//! So this file owns its own process, and asks its two questions **in order**
//! rather than in parallel: first with no world directory configured, then
//! with one. That ordering is the only way to observe both sides of a
//! `OnceLock` at all, and it is deterministic.
//!
//! # What it proves, and why each half is needed
//!
//! `docs/DECISIONS.md` D19's rule for a host implementor is one sentence:
//! *publish a capability only when a production call path exercises it.* The
//! two halves below are that sentence made falsifiable.
//!
//! * **Unpublished when there is nothing behind it.** A server started without
//!   `--world` persists no chunks, so it must not offer to persist a plugin's
//!   files either. A plugin requiring `grit:storage.dir` is refused **by
//!   name**, which is what stops an operator believing their spawn is
//!   protected by claims that will not survive the next restart.
//! * **Published, and backed by a directory that exists and can be written.**
//!   Not "the function returns a `PathBuf`" — the plugin writes a file into
//!   what it was handed and this test reads it back off the disk. A path that
//!   was returned but never created would satisfy a weaker assertion and fail
//!   a real plugin on its first boot.

use std::sync::Mutex;

use grit_plugin_api::capability::well_known as cap;
use grit_plugin_api::event::EventBus;
use grit_plugin_api::host::HostServices;
use grit_plugin_api::{
    load, Capability, CapabilitySet, Manifest, Plugin, PluginError, PluginId, Setup, API_VERSION,
};

use rustmc_server::plugins::Host;
use rustmc_server::tiles::Tiles;

/// A plugin that cannot work without somewhere to write, and proves it by
/// writing there during `enable`.
///
/// Deliberately not `grit:protect`: this crate does not depend on any
/// particular plugin, and what is under test is the *host*, not that plugin.
/// The real `grit:protect` loading into this same host is proved in
/// `grit-plugins/host-proof`.
struct Keeper {
    /// Where `enable` was handed, so the test can check the disk itself
    /// rather than trusting the plugin's word for it.
    saw: std::sync::Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl Plugin for Keeper {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PluginId::parse("grit:keeper").expect("literal"),
            API_VERSION,
        )
        .requires(CapabilitySet::from_strs([cap::STORAGE_DIR]).expect("literal"))
    }

    fn enable(&mut self, setup: &mut Setup<'_>) -> Result<(), PluginError> {
        // `require_storage_dir`, not `storage_dir().ok_or(..)`. The loader has
        // already checked the capability against a set derived from this
        // host's own `storage_root` and has already created the directory, so
        // there is no `None` branch to write.
        let dir = setup.require_storage_dir()?.to_path_buf();
        std::fs::write(dir.join("kept.txt"), "written during enable").map_err(|e| {
            PluginError::new(format!("could not write into {}: {e}", dir.display()))
        })?;
        *self.saw.lock().expect("saw") = Some(dir);
        Ok(())
    }
}

fn storage() -> Capability {
    Capability::parse(cap::STORAGE_DIR).expect("literal")
}

fn load_keeper(host: &Host<'_>) -> (grit_plugin_api::LoadReport, Option<std::path::PathBuf>) {
    let saw = std::sync::Arc::new(Mutex::new(None));
    let plugin = Keeper {
        saw: std::sync::Arc::clone(&saw),
    };
    let mut bus = EventBus::new(host.published_capabilities());
    let report = load(vec![Box::new(plugin) as Box<dyn Plugin>], host, &mut bus);
    let dir = saw.lock().expect("saw").clone();
    (report, dir)
}

#[test]
fn storage_is_published_exactly_when_the_operator_asked_for_persistence() {
    // ---- Half one: no `--world`, so nothing is behind the capability. ----
    let host = Host::detached();
    assert!(
        host.storage_root().is_none(),
        "no world directory has been configured yet"
    );
    assert!(
        !host.published_capabilities().provides(&storage()),
        "a host with nowhere to write must not publish grit:storage.dir"
    );

    let (report, dir) = load_keeper(&host);
    assert!(report.loaded.is_empty(), "nothing may load");
    assert_eq!(report.refused.len(), 1);
    let message = report.refused[0].to_string();
    assert!(
        message.contains("grit:keeper") && message.contains(cap::STORAGE_DIR),
        "the refusal names the plugin and the capability: {message}"
    );
    assert!(
        dir.is_none(),
        "enable never ran, so nothing was written anywhere"
    );

    // ---- Half two: the operator passes `--world`. ----
    let root = std::env::temp_dir().join(format!("rustmc-plugin-storage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("scratch world dir");
    Tiles::persist_to(root.clone());

    let host = Host::detached();
    assert_eq!(
        host.storage_root(),
        Some(root.join("plugins")),
        "plugin storage is rooted under the operator's own world directory"
    );
    assert!(
        host.published_capabilities().provides(&storage()),
        "and now the capability is published, because something backs it"
    );

    let (report, dir) = load_keeper(&host);
    assert!(
        report.refused.is_empty(),
        "refused: {:?}",
        report
            .refused
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    assert_eq!(report.loaded.len(), 1, "the plugin loads");

    // The directory is derived from the plugin id, one component per part, so
    // every host lays out the same tree.
    let expected = root.join("plugins").join("grit").join("keeper");
    assert_eq!(dir.as_deref(), Some(expected.as_path()));
    assert!(
        expected.is_dir(),
        "the loader created it before enable ran: {}",
        expected.display()
    );

    // **Read it back off the disk.** A `PathBuf` that was returned but never
    // created would pass every assertion above this one and fail a real plugin
    // on its first boot.
    let written = std::fs::read_to_string(expected.join("kept.txt")).expect("the plugin's file");
    assert_eq!(written, "written during enable");

    let _ = std::fs::remove_dir_all(&root);
}
