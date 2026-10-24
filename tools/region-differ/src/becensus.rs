//! `becensus` / `bediff` — the block-entity instrument.
//!
//! # Why this exists at all
//!
//! Every worldgen exactness figure this project publishes compares **block
//! states**, and `docs/PARITY.md` §6 says so in one line: *"Anything that is
//! not a block state. Block entities are not written, so no chest contents,
//! no spawner contents, no sign text."* A chest whose block state is right
//! and whose contents are absent scores as a **match** in `diff`, in
//! `census`, and in `ci/admissible-residual.py` alike. That is not a defect
//! in those instruments — they measure what they say they measure — but it
//! does mean **the size of the hole has never been measured by anything**,
//! and an unmeasured hole beside a six-decimal-place percentage is the exact
//! shape this project keeps getting caught by.
//!
//! So: measure first, build second. This module is the measurement, and it
//! is deliberately landed before any generation change so that the number it
//! prints is a property of vanilla rather than a scorecard for us.
//!
//! # What `becensus` counts, and the three questions it answers
//!
//! `ChunkColumn::block_entities` is already parsed and kept as raw NBT (see
//! its doc comment in `rustmc-world`), so nothing new has to be decoded.
//!
//! 1. **How many, by type.** The `id` histogram, straight out of the file.
//! 2. **How many are reachable.** Every entry is attributed to the structure
//!    whose piece bounding box contains it, using the reference's **own**
//!    `structure_starts` NBT — the free oracle `anvil.rs` already documents.
//!    An entry inside no piece box is attributed to `<feature>`, which is the
//!    decoration stage: dungeons, fossils, buried loot and the rest. This is
//!    what converts "vanilla writes N block entities" into "N are reachable
//!    given the structure sets this server actually builds", which is the
//!    honest ceiling on any work here and is **never** the total.
//! 3. **What each type stores.** `--fields` prints, per id, the histogram of
//!    keys its compounds carry. That is how the loot question gets answered
//!    from the data rather than from assumption: a container that saves an
//!    unresolved `LootTable` reference is cheap to reproduce (a string and a
//!    long), and one that saves a resolved `Items` list is not, because
//!    reproducing it means reproducing the whole loot-table roll.
//!
//! # Why attribution uses the whole directory and not the bbox
//!
//! A structure can start outside your window and write pieces into it — the
//! standing rule about structure margins, applied to attribution rather than
//! to generation. So the piece index is built from **every** chunk in the
//! directory and only the *counted* block entities are clipped to `--bbox`.
//! Building the index from the clipped set instead would attribute an
//! in-window chest belonging to an out-of-window mineshaft to `<feature>`,
//! which is precisely the "unbounded census explained a 225-chunk window with
//! blocks from its 362-chunk margin" failure (rule 7) with the sign flipped.
//!
//! # `bediff`, and why a floor is not optional here
//!
//! Two unmodified vanilla runs of one seed do not agree on block states, and
//! there is no reason on earth to assume they agree on block entities: the
//! same threaded decoration order that moves a lake moves the dungeon chest
//! inside it, and a container's saved contents depend on a loot roll taken
//! from a stream the decoration order perturbs. **So `bediff` exists to be
//! run vanilla-against-vanilla before it is ever run against us**, and every
//! figure this module produces is quoted with that floor beside it, never
//! minus it.
//!
//! `bediff` takes **vanilla first**, like every other subcommand here, and
//! for the same reason: backwards inverts the sign of `missing` and
//! `spurious`, which are the two numbers anyone actually reads.
//!
//! It classifies each position — the union of the two sides' block-entity
//! coordinates — into exactly one bucket:
//!
//! * `missing`   — in vanilla, absent from ours;
//! * `spurious`  — in ours, absent from vanilla;
//! * `id`        — both present, different `id`;
//! * `payload`   — same `id`, different NBT below it;
//! * `exact`     — identical compounds.
//!
//! The buckets partition the union, so `missing + spurious + id + payload +
//! exact` is the denominator and no position is charged twice.

use crate::{load_dir, parse_bbox};
use rustmc_world::anvil::ChunkColumn;
use rustmc_world::nbt::Nbt;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// The key a block entity is filed under: its world position.
type Pos = (i32, i32, i32);

/// `x`/`y`/`z`/`id` are the entry's identity, not its payload. They are held
/// out of the payload comparison because they are already the comparison key
/// and the id already has its own bucket — leaving them in would double-count
/// an id mismatch as a payload mismatch too.
const IDENTITY_KEYS: [&str; 4] = ["x", "y", "z", "id"];

/// Pull `(pos, id, compound)` for every block entity in a chunk.
///
/// An entry missing any of `x`/`y`/`z`/`id` is skipped and counted as
/// malformed by the caller rather than silently dropped: a reference that
/// contains one is a fact about the harvest, and a census that hides it is a
/// census measuring its own tolerance.
fn entries(chunk: &ChunkColumn) -> (Vec<(Pos, String, &Nbt)>, u64) {
    let mut out = Vec::new();
    let mut malformed = 0u64;
    for e in &chunk.block_entities {
        let (Some(x), Some(y), Some(z), Some(id)) = (
            e.get("x").and_then(Nbt::as_i32),
            e.get("y").and_then(Nbt::as_i32),
            e.get("z").and_then(Nbt::as_i32),
            e.get("id").and_then(Nbt::as_str),
        ) else {
            malformed += 1;
            continue;
        };
        out.push(((x, y, z), id.to_string(), e));
    }
    (out, malformed)
}

/// Every structure piece box in the directory, as `(structure id, box)`.
///
/// Built from all chunks, unclipped — see the module header for why.
fn piece_index(chunks: &BTreeMap<(i32, i32), ChunkColumn>) -> Vec<(String, [i32; 6])> {
    let mut out = Vec::new();
    for chunk in chunks.values() {
        for start in &chunk.structure_starts {
            for piece in &start.pieces {
                out.push((start.id.clone(), piece.bounding_box));
            }
        }
    }
    out
}

/// Which structure owns this position, or `None` for the decoration stage.
///
/// A position inside two structures' boxes is attributed to **all** of them
/// by the caller taking the sorted set, because picking one would invent a
/// precedence vanilla does not have and would make the per-structure columns
/// silently not sum to the total. The `+overlap` line in the output is how
/// often that happens, so a reader can tell whether the columns are a
/// partition or merely a cover.
fn owners(index: &[(String, [i32; 6])], (x, y, z): Pos) -> BTreeSet<&str> {
    index
        .iter()
        .filter(|(_, b)| x >= b[0] && x <= b[3] && y >= b[1] && y <= b[4] && z >= b[2] && z <= b[5])
        .map(|(id, _)| id.as_str())
        .collect()
}

/// Sorted `key:type` list for one compound, identity keys included so that
/// a type which omits `id` shows up rather than looking like every other.
fn field_shape(e: &Nbt) -> Vec<String> {
    let Some(m) = e.as_compound() else {
        return Vec::new();
    };
    m.iter()
        .map(|(k, v)| format!("{k}:{}", tag_name(v)))
        .collect()
}

/// A canonical one-line text form of a tag, for `--raw`.
///
/// Deliberately lossy about float formatting and deliberately NOT used for
/// any comparison — comparison is `Nbt`'s own `PartialEq` on the parsed
/// values, so a rendering bug here can misinform a reader but can never move
/// a number.
fn render(v: &Nbt) -> String {
    match v {
        Nbt::Byte(x) => format!("{x}b"),
        Nbt::Short(x) => format!("{x}s"),
        Nbt::Int(x) => format!("{x}"),
        Nbt::Long(x) => format!("{x}L"),
        Nbt::Float(x) => format!("{x}f"),
        Nbt::Double(x) => format!("{x}d"),
        Nbt::ByteArray(a) => format!("[B;{} entries]", a.len()),
        Nbt::String(s) => format!("{s:?}"),
        Nbt::IntArray(a) => format!("[I;{a:?}]"),
        Nbt::LongArray(a) => format!("[L;{a:?}]"),
        Nbt::List(items) => {
            let inner: Vec<String> = items.iter().map(render).collect();
            format!("[{}]", inner.join(","))
        }
        Nbt::Compound(m) => {
            let inner: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}:{}", render(v)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
    }
}

fn tag_name(v: &Nbt) -> &'static str {
    match v {
        Nbt::Byte(_) => "byte",
        Nbt::Short(_) => "short",
        Nbt::Int(_) => "int",
        Nbt::Long(_) => "long",
        Nbt::Float(_) => "float",
        Nbt::Double(_) => "double",
        Nbt::ByteArray(_) => "bytes",
        Nbt::String(_) => "string",
        Nbt::List(_) => "list",
        Nbt::Compound(_) => "compound",
        Nbt::IntArray(_) => "ints",
        Nbt::LongArray(_) => "longs",
    }
}

pub fn cmd_becensus(args: &[String]) -> io::Result<()> {
    let mut counted = load_dir(Path::new(&args[1]))?;
    // The index is taken BEFORE the clip and is deliberately not re-derived
    // from `counted`: a start outside the window still owns pieces inside it.
    let index = piece_index(&counted);
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        counted.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    let want_fields = args.iter().any(|s| s == "--fields");
    let positions_out = args.iter().any(|s| s == "--positions");
    // `--raw N` prints the full compound of the first N kept entries. A field
    // *shape* says a chest stores `LootTable:string`; only the value says
    // whether it is an unresolved reference or a resolved item list, and that
    // is the difference between a cheap port and an expensive one.
    let raw_limit: usize = args
        .iter()
        .position(|s| s == "--raw")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut raw_shown = 0usize;
    let filter = args
        .iter()
        .position(|s| s == "--match")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let mut by_id: BTreeMap<String, u64> = BTreeMap::new();
    // id -> owner -> count. `<feature>` is the decoration stage.
    let mut by_id_owner: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut by_owner: BTreeMap<String, u64> = BTreeMap::new();
    let mut fields: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut total = 0u64;
    let mut malformed_total = 0u64;
    let mut overlap = 0u64;

    for chunk in counted.values() {
        let (es, malformed) = entries(chunk);
        malformed_total += malformed;
        for (pos, id, tag) in es {
            if let Some(want) = &filter {
                if !id.contains(want.as_str()) {
                    continue;
                }
            }
            total += 1;
            *by_id.entry(id.clone()).or_insert(0) += 1;
            let os = owners(&index, pos);
            if os.len() > 1 {
                overlap += 1;
            }
            let names: Vec<String> = if os.is_empty() {
                vec!["<feature>".to_string()]
            } else {
                os.iter().map(|s| (*s).to_string()).collect()
            };
            for n in &names {
                *by_owner.entry(n.clone()).or_insert(0) += 1;
                *by_id_owner
                    .entry(id.clone())
                    .or_default()
                    .entry(n.clone())
                    .or_insert(0) += 1;
            }
            if want_fields {
                let f = fields.entry(id.clone()).or_default();
                for k in field_shape(tag) {
                    *f.entry(k).or_insert(0) += 1;
                }
            }
            if positions_out {
                println!("{} {} {} {id}", pos.0, pos.1, pos.2);
            }
            if raw_shown < raw_limit {
                raw_shown += 1;
                println!("RAW {} {} {} {}", pos.0, pos.1, pos.2, render(tag));
            }
        }
    }

    println!(
        "BECENSUS chunks={} block_entities={total} malformed={malformed_total} pieces={} multi_owned={overlap}",
        counted.len(),
        index.len()
    );

    println!("--- by id ---");
    let mut rows: Vec<(&String, &u64)> = by_id.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (id, n) in &rows {
        println!("{n:>8}  {id}");
    }

    println!("--- by owner (a structure start whose piece box contains it, else <feature>) ---");
    let mut orows: Vec<(&String, &u64)> = by_owner.iter().collect();
    orows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (o, n) in &orows {
        println!("{n:>8}  {o}");
    }

    println!("--- id x owner ---");
    for (id, n) in &rows {
        let per = &by_id_owner[*id];
        let mut prow: Vec<(&String, &u64)> = per.iter().collect();
        prow.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let cells: Vec<String> = prow.iter().map(|(o, c)| format!("{o}={c}")).collect();
        println!("{n:>8}  {id}  {}", cells.join(" "));
    }

    if want_fields {
        println!("--- fields (key:type -> how many entries of that id carry it) ---");
        for (id, n) in &rows {
            let f = &fields[*id];
            let mut frow: Vec<(&String, &u64)> = f.iter().collect();
            frow.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let cells: Vec<String> = frow.iter().map(|(k, c)| format!("{k}={c}")).collect();
            println!("{n:>8}  {id}  {}", cells.join(" "));
        }
    }
    Ok(())
}

/// `bereach <vanilla-dir> <ours-dir>` — the reachability ceiling, positionally.
///
/// # Why a count comparison is not this number
///
/// It is tempting to read the ceiling off two `census` runs: vanilla places 32
/// `potent_sulfur` and we place 31, so 31 are reachable. That is wrong in the
/// way this project keeps getting caught by — **two counts can agree while
/// every position differs.** A feature displaced by one chunk scores a perfect
/// count and a zero overlap.
///
/// So this walks vanilla's block-entity positions and asks, at each one, what
/// block *we* have there. A position is **reachable** when our block name
/// equals vanilla's block name at that same position: the block entity has
/// somewhere to attach, and writing it is a matter of emitting NBT rather than
/// of building a structure first. Anything else is out of reach until the
/// thing that places that block lands, and no amount of block-entity work
/// moves it.
///
/// The expected block per block-entity type is read off **vanilla at that
/// position**, not from a table in this file. A hardcoded `chest ->
/// minecraft:chest` map would be one more thing to go stale, and it would
/// quietly decide the answer it is supposed to measure.
pub fn cmd_bereach(args: &[String]) -> io::Result<()> {
    let mut van = load_dir(Path::new(&args[1]))?;
    let mut ours = load_dir(Path::new(&args[2]))?;
    let index = piece_index(&van);
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        van.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
        ours.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    let positions_out = args.iter().any(|s| s == "--positions");

    // id -> (reachable, out of reach). Owner columns alongside, because "how
    // much is out of reach" is only actionable with "and what would have to
    // be built for it not to be".
    let mut reach: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut owner_reach: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    // What we have instead, where we do not have the right block.
    let mut instead: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0u64;
    let mut reachable = 0u64;
    let mut absent_chunk = 0u64;

    for (key, chunk) in &van {
        let Some(mine) = ours.get(key) else {
            absent_chunk += u64::try_from(entries(chunk).0.len()).unwrap_or(0);
            continue;
        };
        for (pos, id, _) in entries(chunk).0 {
            total += 1;
            let want = chunk.block_at(pos.0, pos.1, pos.2).unwrap_or("<none>");
            let got = mine.block_at(pos.0, pos.1, pos.2).unwrap_or("<none>");
            // Block NAMES, not states: a chest facing the wrong way is a
            // block-state defect that belongs to the block-state figure, and
            // charging it here would double-count it and would also hide the
            // thing this instrument is for.
            let wn = want.split('[').next().unwrap_or(want);
            let gn = got.split('[').next().unwrap_or(got);
            let ok = wn == gn;
            let os = owners(&index, pos);
            let names: Vec<String> = if os.is_empty() {
                vec!["<feature>".to_string()]
            } else {
                os.iter().map(|s| (*s).to_string()).collect()
            };
            let e = reach.entry(id.clone()).or_insert((0, 0));
            if ok {
                reachable += 1;
                e.0 += 1;
            } else {
                e.1 += 1;
                *instead.entry(format!("{wn} -> {gn}")).or_insert(0) += 1;
            }
            for n in names {
                let oe = owner_reach.entry(n).or_insert((0, 0));
                if ok {
                    oe.0 += 1;
                } else {
                    oe.1 += 1;
                }
            }
            if positions_out {
                println!(
                    "{} {} {} {} {id} {wn} {gn}",
                    if ok { "REACHABLE" } else { "OUTOFREACH" },
                    pos.0,
                    pos.1,
                    pos.2
                );
            }
        }
    }

    println!(
        "BEREACH vanilla_block_entities={total} reachable={reachable} out_of_reach={} chunks_absent_from_ours={absent_chunk}",
        total - reachable
    );
    if total > 0 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "counts here are in the hundreds"
        )]
        let pct = reachable as f64 / total as f64 * 100.0;
        println!("  reachable share {pct:.3}%  ({reachable} of {total})");
    }
    println!("--- by id: reachable / out of reach ---");
    let mut rows: Vec<(&String, &(u64, u64))> = reach.iter().collect();
    rows.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)).then(a.0.cmp(b.0)));
    for (id, (r, o)) in rows {
        println!("{r:>8} / {o:<8} {id}");
    }
    println!("--- by owner: reachable / out of reach ---");
    let mut orows: Vec<(&String, &(u64, u64))> = owner_reach.iter().collect();
    orows.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)).then(a.0.cmp(b.0)));
    for (o, (r, n)) in orows {
        println!("{r:>8} / {n:<8} {o}");
    }
    if !instead.is_empty() {
        println!("--- where the block is out of reach, what we have instead ---");
        let mut irows: Vec<(&String, &u64)> = instead.iter().collect();
        irows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (k, n) in irows {
            println!("{n:>8}  {k}");
        }
    }
    Ok(())
}

/// Strip identity keys, then compare what is left.
fn payload(e: &Nbt, ignore: &[String]) -> BTreeMap<String, Nbt> {
    let Some(m) = e.as_compound() else {
        return BTreeMap::new();
    };
    m.iter()
        .filter(|(k, _)| !IDENTITY_KEYS.contains(&k.as_str()) && !ignore.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// `bediff <vanilla-dir> <ours-dir>` — VANILLA FIRST.
///
/// Only chunks present as `Status=full` in **both** directories are compared;
/// a chunk one side never generated is not evidence about block entities in
/// either direction, and including it would charge the whole of a missing
/// chunk's contents to the side that has fewer chunks.
pub fn cmd_bediff(args: &[String]) -> io::Result<()> {
    let mut a = load_dir(Path::new(&args[1]))?;
    let mut b = load_dir(Path::new(&args[2]))?;
    if let Some((x1, z1, x2, z2)) = parse_bbox(args) {
        a.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
        b.retain(|&(x, z), _| x >= x1 && x <= x2 && z >= z1 && z <= z2);
    }
    let common: Vec<(i32, i32)> = a.keys().filter(|k| b.contains_key(k)).copied().collect();
    let positions_out = args.iter().any(|s| s == "--positions");
    let mut ignore: Vec<String> = Vec::new();
    for (i, s) in args.iter().enumerate() {
        if s == "--ignore-key" {
            if let Some(k) = args.get(i + 1) {
                ignore.push(k.clone());
            }
        }
    }

    // `--match <id-substr>` restricts the whole comparison to one class.
    //
    // THE REASON THIS IS NOT A COSMETIC FILTER: a floor belongs to the thing
    // it bounds. The all-types floor on this window is 0..13, but 100% of a
    // single class's charge can sit inside a floor that is mostly some other
    // class — or, worse, outside a floor that looks generous only because a
    // noisier class inflated it. `docs/DETERMINISM.md` §18 makes the same
    // point for block states: the window's aggregate floor is unusable while
    // `grass_block -> dirt` on the same window has a floor of 1. A per-class
    // charge must be quoted against a per-class floor, and this is what
    // makes taking one cost nothing.
    let filter = args
        .iter()
        .position(|s| s == "--match")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let side = |chunks: &BTreeMap<(i32, i32), ChunkColumn>| {
        let mut m: BTreeMap<Pos, (String, Nbt)> = BTreeMap::new();
        for k in &common {
            let (es, _) = entries(&chunks[k]);
            for (pos, id, tag) in es {
                if let Some(want) = &filter {
                    if !id.contains(want.as_str()) {
                        continue;
                    }
                }
                m.insert(pos, (id, tag.clone()));
            }
        }
        m
    };
    let va = side(&a);
    let ou = side(&b);

    let mut missing = 0u64;
    let mut spurious = 0u64;
    let mut id_mismatch = 0u64;
    let mut payload_mismatch = 0u64;
    let mut exact = 0u64;
    // Which id each bucket falls on, so a number turns into a work list.
    let mut miss_by_id: BTreeMap<String, u64> = BTreeMap::new();
    let mut spur_by_id: BTreeMap<String, u64> = BTreeMap::new();
    let mut pay_by_id: BTreeMap<String, u64> = BTreeMap::new();
    // Which KEY under a matching id disagrees — the work list one level down.
    let mut pay_by_key: BTreeMap<String, u64> = BTreeMap::new();

    let all_pos: BTreeSet<Pos> = va.keys().chain(ou.keys()).copied().collect();
    for pos in &all_pos {
        match (va.get(pos), ou.get(pos)) {
            (Some((ia, ta)), Some((ib, tb))) => {
                if ia != ib {
                    id_mismatch += 1;
                    if positions_out {
                        println!("ID {} {} {} {ia} -> {ib}", pos.0, pos.1, pos.2);
                    }
                    continue;
                }
                let pa = payload(ta, &ignore);
                let pb = payload(tb, &ignore);
                if pa == pb {
                    exact += 1;
                } else {
                    payload_mismatch += 1;
                    *pay_by_id.entry(ia.clone()).or_insert(0) += 1;
                    for k in pa.keys().chain(pb.keys()).collect::<BTreeSet<_>>() {
                        if pa.get(k) != pb.get(k) {
                            *pay_by_key.entry(format!("{ia}.{k}")).or_insert(0) += 1;
                        }
                    }
                    if positions_out {
                        println!("PAYLOAD {} {} {} {ia}", pos.0, pos.1, pos.2);
                    }
                }
            }
            (Some((ia, _)), None) => {
                missing += 1;
                *miss_by_id.entry(ia.clone()).or_insert(0) += 1;
                if positions_out {
                    println!("MISSING {} {} {} {ia}", pos.0, pos.1, pos.2);
                }
            }
            (None, Some((ib, _))) => {
                spurious += 1;
                *spur_by_id.entry(ib.clone()).or_insert(0) += 1;
                if positions_out {
                    println!("SPURIOUS {} {} {} {ib}", pos.0, pos.1, pos.2);
                }
            }
            (None, None) => unreachable!("position came from one of the two maps"),
        }
    }

    let denom = all_pos.len() as u64;
    println!(
        "BEDIFF chunks={} vanilla_be={} ours_be={} union={denom}",
        common.len(),
        va.len(),
        ou.len()
    );
    println!(
        "  missing={missing} spurious={spurious} id={id_mismatch} payload={payload_mismatch} exact={exact}"
    );
    if denom > 0 {
        let pct = f64::from(u32::try_from(exact).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(denom).unwrap_or(u32::MAX))
            * 100.0;
        println!("  exactness {pct:.6}%  ({exact} of {denom})");
    }
    let dump = |label: &str, m: &BTreeMap<String, u64>| {
        if m.is_empty() {
            return;
        }
        let mut r: Vec<(&String, &u64)> = m.iter().collect();
        r.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!("--- {label} ---");
        for (k, n) in r {
            println!("{n:>8}  {k}");
        }
    };
    dump("missing by id", &miss_by_id);
    dump("spurious by id", &spur_by_id);
    dump("payload mismatch by id", &pay_by_id);
    dump("payload mismatch by id.key", &pay_by_key);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be(x: i32, y: i32, z: i32, id: &str, extra: &[(&str, Nbt)]) -> Nbt {
        let mut m = BTreeMap::new();
        m.insert("x".into(), Nbt::Int(x));
        m.insert("y".into(), Nbt::Int(y));
        m.insert("z".into(), Nbt::Int(z));
        m.insert("id".into(), Nbt::String(id.into()));
        for (k, v) in extra {
            m.insert((*k).into(), v.clone());
        }
        Nbt::Compound(m)
    }

    /// An entry inside a piece box is that structure's; one outside every box
    /// belongs to the decoration stage. This is the whole reachability
    /// argument, so it is the thing worth a test.
    #[test]
    fn attribution_is_by_piece_box_not_by_chunk() {
        let index = vec![
            ("minecraft:mineshaft".to_string(), [0, 0, 0, 15, 15, 15]),
            (
                "minecraft:village_plains".to_string(),
                [8, 8, 8, 31, 31, 31],
            ),
        ];
        assert_eq!(
            owners(&index, (1, 1, 1)).into_iter().collect::<Vec<_>>(),
            vec!["minecraft:mineshaft"]
        );
        assert!(
            owners(&index, (100, 1, 100)).is_empty(),
            "outside every box"
        );
        // An overlap is reported to both, deliberately.
        assert_eq!(owners(&index, (10, 10, 10)).len(), 2);
    }

    /// A piece box whose start sits far outside the counted window still owns
    /// what it wrote inside it — the reason `piece_index` is built from the
    /// unclipped directory. Asserted on the index directly rather than by
    /// constructing a `ChunkColumn`: the property under test is that
    /// containment is by *box*, and a box reaching in from -640 is the case
    /// a bbox-clipped index would silently lose to `<feature>`.
    #[test]
    fn a_start_outside_the_window_still_owns_its_pieces_inside_it() {
        let index = vec![(
            "minecraft:mineshaft".to_string(),
            [-640, 10, -640, 640, 20, 640],
        )];
        assert_eq!(owners(&index, (0, 15, 0)).len(), 1);
        // ... and does not reach positions genuinely outside it.
        assert!(owners(&index, (0, 40, 0)).is_empty());
    }

    /// The buckets must partition the union, or the denominator is a lie.
    /// Tested here on the classifier's inputs rather than through the CLI.
    #[test]
    fn identity_keys_are_held_out_of_the_payload() {
        let a = be(
            1,
            2,
            3,
            "minecraft:chest",
            &[("LootTable", Nbt::String("x".into()))],
        );
        let b = be(
            9,
            9,
            9,
            "minecraft:chest",
            &[("LootTable", Nbt::String("x".into()))],
        );
        assert_eq!(
            payload(&a, &[]),
            payload(&b, &[]),
            "position is not payload"
        );
        let c = be(
            1,
            2,
            3,
            "minecraft:chest",
            &[("LootTable", Nbt::String("y".into()))],
        );
        assert_ne!(payload(&a, &[]), payload(&c, &[]), "contents are payload");
    }

    #[test]
    fn malformed_entries_are_counted_not_dropped_silently() {
        let mut chunk = ChunkColumn {
            x: 0,
            z: 0,
            status: "minecraft:full".into(),
            data_version: 0,
            sections: Vec::new(),
            is_light_on: false,
            light: std::collections::BTreeMap::new(),
            block_entities: vec![
                be(1, 2, 3, "minecraft:chest", &[]),
                Nbt::Compound(BTreeMap::new()),
            ],
            heightmaps: BTreeMap::new(),
            structure_starts: Vec::new(),
            structure_references: BTreeMap::new(),
            extra: rustmc_world::anvil::generated_extra(),
        };
        let (es, malformed) = entries(&chunk);
        assert_eq!(es.len(), 1);
        assert_eq!(malformed, 1);
        chunk.block_entities.clear();
        assert_eq!(entries(&chunk).1, 0);
    }
}
