//! The pool and template registry `JigsawPlacement` walks over.
//!
//! `crates/rustmc-worldgen`'s assembler is pure: it takes a
//! [`rustmc_worldgen::structure::jigsaw::Library`] and never touches a file.
//! This is the file half — Mojang's datagen tree under
//! [`crate::DATA`], the same tree `ocean_ruin` already reads its templates
//! from, plus the templates the jar carries under `data/minecraft/structure`.
//!
//! Everything is cached, and the caching is not just for speed. A pool is
//! **weight-expanded** at load — a 550-entry town-centre pool becomes 550
//! `Arc`s — and `getShuffledTemplates` clones that list on every jigsaw block
//! of every piece. Loading and expanding it once per name is the difference
//! between a village that assembles in milliseconds and one that re-parses
//! 188 JSON files a few thousand times.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustmc_worldgen::structure::jigsaw::{Element, ElementKind, Library, Pool};
use rustmc_worldgen::structure::template::Template;
use rustmc_worldgen::structure::template_pool::{PoolElement, TemplatePool};

/// How much work the file half of the library has actually done.
///
/// # Why a counter and not a timer
///
/// The doc comment at the top of this file says the caching is "the
/// difference between a village that assembles in milliseconds and one that
/// re-parses 188 JSON files a few thousand times". That is a claim about a
/// **count**, and until this existed nothing in the tree could tell whether it
/// held: every parity test here compares values, and a cache that has been
/// defeated by its caller produces exactly the right values. `perf` cannot
/// answer it either — the parsing is spread over `serde_json`, `flate2` and
/// `Template::from_nbt`, none of which name a pool.
///
/// So the instrument is the count of loads against the count of *distinct
/// names*, which is what vanilla's per-server registry would pay. They should
/// be equal. `RUSTMC_LIBRARY_LOADS=1` prints them.
pub mod loads {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub static POOL_LOADS: AtomicU64 = AtomicU64::new(0);
    pub static POOL_LOAD_NANOS: AtomicU64 = AtomicU64::new(0);
    pub static TEMPLATE_LOADS: AtomicU64 = AtomicU64::new(0);
    pub static TEMPLATE_LOAD_NANOS: AtomicU64 = AtomicU64::new(0);
    pub static LIBRARIES: AtomicU64 = AtomicU64::new(0);
    /// `getFirstFreeHeight` calls out of a jigsaw walk. Each is a full
    /// beardless noise column, which is the single most expensive thing a
    /// structure layout can ask for, and nothing counted them.
    pub static HEIGHT_CALLS: AtomicU64 = AtomicU64::new(0);
    /// Chunks the placement grid accepted, and jigsaw walks run on them.
    pub static PLACEMENT_CHUNKS: AtomicU64 = AtomicU64::new(0);
    pub static ATTEMPTS: AtomicU64 = AtomicU64::new(0);
    pub static HEIGHT_NANOS: AtomicU64 = AtomicU64::new(0);
    /// The **distinct** columns asked for, recorded only when the report is
    /// switched on because a set insert on every call would perturb the
    /// timing it sits beside.
    ///
    /// This is the falsifiable half of the counter: `HEIGHT_CALLS` alone
    /// cannot say whether a column was computed once or forty times, and
    /// "how many times do we evaluate something vanilla evaluates once" is
    /// the question that pays out here.
    pub static HEIGHT_DISTINCT: std::sync::Mutex<Option<std::collections::BTreeSet<(i32, i32)>>> =
        std::sync::Mutex::new(None);

    /// Whether to record distinct columns — read once, because a `var_os` per
    /// height call would itself cost more than the set insert.
    pub fn recording() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("RUSTMC_LIBRARY_LOADS").is_some())
    }

    /// `(libraries, pool loads, pool seconds, template loads, template
    /// seconds)` since the process started.
    #[must_use]
    pub fn read() -> (u64, u64, f64, u64, f64) {
        (
            LIBRARIES.load(Ordering::Relaxed),
            POOL_LOADS.load(Ordering::Relaxed),
            POOL_LOAD_NANOS.load(Ordering::Relaxed) as f64 / 1e9,
            TEMPLATE_LOADS.load(Ordering::Relaxed),
            TEMPLATE_LOAD_NANOS.load(Ordering::Relaxed) as f64 / 1e9,
        )
    }

    /// Print the tally if `RUSTMC_LIBRARY_LOADS` asks.
    ///
    /// The pool seconds are **not** additive with the template seconds: a pool
    /// load loads the templates it names, so the template time is inside it.
    /// Said here because a reader who adds them gets a number larger than the
    /// stage they came from and concludes the instrument is broken.
    pub fn report() {
        if std::env::var_os("RUSTMC_LIBRARY_LOADS").is_none() {
            return;
        }
        let (libs, pools, pool_s, templates, template_s) = read();
        eprintln!(
            "library {libs} libraries built, {pools} pool loads ({pool_s:.2} s, \
             templates included), {templates} template loads ({template_s:.2} s)"
        );
        eprintln!(
            "library {} placement chunks, {} jigsaw attempts, {} first_free_height \
             calls ({:.2} s)",
            PLACEMENT_CHUNKS.load(Ordering::Relaxed),
            ATTEMPTS.load(Ordering::Relaxed),
            HEIGHT_CALLS.load(Ordering::Relaxed),
            HEIGHT_NANOS.load(Ordering::Relaxed) as f64 / 1e9,
        );
        let distinct = HEIGHT_DISTINCT
            .lock()
            .expect("the distinct-column set lock")
            .as_ref()
            .map_or(0, std::collections::BTreeSet::len);
        eprintln!(
            "library {distinct} distinct columns among those calls — the ratio is how \
             many times one column is recomputed"
        );
    }
}

/// A pool/template registry backed by the extract, plus a heightmap source.
pub struct FileLibrary<H> {
    pools: BTreeMap<String, Option<Arc<Pool>>>,
    templates: BTreeMap<String, Option<Arc<Template>>>,
    /// `ChunkGenerator.getFirstFreeHeight(x, z, WORLD_SURFACE_WG, ..)`.
    pub height: H,
    /// Names that could not be resolved, so a caller can report a thin village
    /// rather than silently generating one.
    pub missing: Vec<String>,
    /// `PoolAliasLookup` — `alias -> target`, empty for every structure but
    /// `trial_chambers`. Applied here because this is the one place a pool
    /// name becomes a pool, which is where vanilla applies it too: all three
    /// `PoolAliasLookup.lookup` call sites in `JigsawPlacement` are pool
    /// resolutions (the start pool, a jigsaw block's target pool, and the
    /// fallback).
    aliases: BTreeMap<String, String>,
}

impl<H: FnMut(i32, i32) -> i32> FileLibrary<H> {
    pub fn new(height: H) -> Self {
        loads::LIBRARIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            pools: BTreeMap::new(),
            templates: BTreeMap::new(),
            height,
            missing: Vec::new(),
            aliases: BTreeMap::new(),
        }
    }

    /// Resolve every pool name through `table` for the rest of this library's
    /// life.
    ///
    /// The cache is keyed on the name *as asked for*, so a library that is
    /// given a table after it has already answered for an alias would return
    /// the stale answer. Nothing does that — the table is installed before the
    /// walk starts — and the `pools.clear()` makes the ordering irrelevant
    /// rather than relying on a caller remembering it.
    #[must_use]
    pub fn with_aliases(mut self, table: BTreeMap<String, String>) -> Self {
        self.aliases = table;
        self.pools.clear();
        self
    }

    fn template(&mut self, name: &str) -> Option<Arc<Template>> {
        if let Some(hit) = self.templates.get(name) {
            return hit.clone();
        }
        let rel = name.strip_prefix("minecraft:").unwrap_or(name);
        let path = crate::data_path(&format!("structure/{rel}.nbt"));
        let clock = std::time::Instant::now();
        let loaded = if path.exists() {
            std::fs::File::open(&path).ok().and_then(|file| {
                let mut reader = flate2::read::GzDecoder::new(file);
                let (_n, root) = rustmc_world::nbt::read_named_root(&mut reader).ok()?;
                Template::from_nbt(&root).ok().map(Arc::new)
            })
        } else if crate::data_path("structure").is_dir() {
            // `getOrCreate`: a name the jar has no file for is an empty
            // template, not a refusal. The extract is complete against the
            // jar's manifest (`structure_census`), so a file absent from a
            // present extract is absent from the jar. A missing *extract*
            // still refuses.
            Some(Arc::new(Template::empty()))
        } else {
            None
        };
        loads::TEMPLATE_LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        loads::TEMPLATE_LOAD_NANOS.fetch_add(
            clock.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        if loaded.is_none() {
            self.missing.push(format!("template {name}"));
        }
        self.templates.insert(name.to_string(), loaded.clone());
        loaded
    }

    /// One pool element's walk-side shape. `None` when a template it names
    /// is missing, which refuses the whole pool rather than building a piece
    /// that is not vanilla's.
    fn element_kind(&mut self, element: &PoolElement) -> Option<ElementKind> {
        Some(match element {
            PoolElement::Single {
                location,
                processors,
                legacy,
                ..
            } => {
                let template = self.template(location)?;
                ElementKind::Single {
                    location: location.clone(),
                    template,
                    processors: processors.clone(),
                    legacy: *legacy,
                }
            }
            PoolElement::Feature { feature, .. } => ElementKind::Feature {
                feature: feature.clone(),
            },
            PoolElement::Empty => ElementKind::Empty,
            // `ListPoolElement`'s constructor calls `setProjectionOnEachElement`,
            // so every child places with the list's projection, not its own.
            PoolElement::List {
                elements,
                projection,
            } => {
                let mut children = Vec::new();
                for child in elements {
                    children.push(Element {
                        kind: self.element_kind(child)?,
                        projection: *projection,
                    });
                }
                if children.is_empty() {
                    return None;
                }
                ElementKind::List { elements: children }
            }
        })
    }

    fn load_pool(&mut self, name: &str) -> Option<Arc<Pool>> {
        let clock = std::time::Instant::now();
        loads::POOL_LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let out = self.load_pool_uncounted(name);
        loads::POOL_LOAD_NANOS.fetch_add(
            clock.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        out
    }

    fn load_pool_uncounted(&mut self, name: &str) -> Option<Arc<Pool>> {
        let rel = name.strip_prefix("minecraft:").unwrap_or(name);
        let path = crate::data_path(&format!("worldgen/template_pool/{rel}.json"));
        let text = std::fs::read_to_string(&path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&text).ok()?;
        let parsed = TemplatePool::parse(&json).ok()?;
        let mut elements = Vec::new();
        for entry in &parsed.elements {
            let kind = self.element_kind(&entry.element)?;
            let element = Arc::new(Element {
                kind,
                projection: entry.element.projection(),
            });
            // The constructor's expansion: `for (i = 0; i < weight; i++)
            // templates.add(element)`. The *count* is what both the uniform
            // draw and the shuffle's length depend on.
            for _ in 0..entry.weight.max(0) {
                elements.push(element.clone());
            }
        }
        Some(Arc::new(Pool {
            name: name.to_string(),
            fallback: parsed.fallback.clone(),
            elements,
        }))
    }
}

impl<H: FnMut(i32, i32) -> i32> Library for FileLibrary<H> {
    fn pool(&mut self, name: &str) -> Option<Arc<Pool>> {
        if let Some(hit) = self.pools.get(name) {
            return hit.clone();
        }
        // `PoolAliasLookup.lookup` is `getOrDefault(key, key)`: a name that is
        // not an alias resolves to itself.
        let resolved = self
            .aliases
            .get(name)
            .map_or(name, String::as_str)
            .to_string();
        let loaded = self.load_pool(&resolved);
        if loaded.is_none() {
            self.missing.push(format!("pool {resolved}"));
        }
        self.pools.insert(name.to_string(), loaded.clone());
        loaded
    }

    fn first_free_height(&mut self, x: i32, z: i32) -> i32 {
        loads::HEIGHT_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if loads::recording() {
            loads::HEIGHT_DISTINCT
                .lock()
                .expect("the distinct-column set lock")
                .get_or_insert_with(std::collections::BTreeSet::new)
                .insert((x, z));
        }
        let clock = std::time::Instant::now();
        let out = (self.height)(x, z);
        loads::HEIGHT_NANOS.fetch_add(
            clock.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        out
    }
}
