//! oracle-runner — rustmc's gameplay oracle.
//!
//! `region-differ` answers "does our world have vanilla's blocks in it".
//! This answers the other half: **does our server behave the way vanilla
//! behaves, as seen by a client**. It runs the same scripted scenario
//! against real vanilla and against rustmc and diffs what each one said on
//! the wire.
//!
//! ```text
//!   oracle-runner status  <host:port>
//!   oracle-runner join    <host:port> <name> [--stay-secs N]
//!   oracle-runner check   <host:port> <name> <checks.toml>
//!   oracle-runner record  <host:port> <scenario-dir> --out <trace.json>
//!                         [--console <console.fifo>]
//!   oracle-runner compare <scenario-dir> --reference <a.json> --subject <b.json>
//!                         [--baseline <b.toml>] [--json <out.json>]
//!   oracle-runner families
//! ```
//!
//! The design decisions worth knowing before reading further live in the
//! modules that make them: [`scenario`] for what a check is, [`trace`] for
//! what is recorded and why nothing is thrown away at record time, and
//! [`compare`] for the verdict that separates "we did it differently" from
//! "we never implemented it".

mod client;
mod compare;
mod pitch;
mod record;
mod scenario;
mod trace;

use client::Tables;
use compare::{Baseline, Counts, Verdict};
use scenario::Scenario;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("status") if args.len() >= 2 => cmd_status(&args[1]),
        Some("join") if args.len() >= 3 => {
            let stay = flag(&args, "--stay-secs")
                .and_then(|v| v.parse().ok())
                .unwrap_or(5);
            cmd_join(&args[1], &args[2], stay)
        }
        Some("check") if args.len() >= 4 => cmd_check(&args[1], &args[2], &args[3]),
        Some("record") if args.len() >= 3 => cmd_record(&args[1], &args[2], &args),
        Some("dump-recipe-displays") if args.len() >= 3 => {
            cmd_dump_recipe_displays(&args[1], &args[2], &args)
        }
        Some("compare") if args.len() >= 2 => cmd_compare(&args[1], &args),
        Some("compare-serve") if args.len() >= 2 => cmd_compare_serve(&args[1], &args),
        Some("explain") if args.len() >= 2 => cmd_explain(&args[1], &args),
        Some("project") if args.len() >= 2 => cmd_project(&args[1], &args),
        // Takes no scenario directory and reads no trace: the projection is a
        // property of this binary, and the whole point of the stamp is that it
        // can be re-derived on a tree that has nothing else set up.
        Some("projection-id") => {
            if args.iter().any(|a| a == "--explain") {
                print!("{}", compare::projection_explain());
            } else {
                println!("{}", compare::projection_identity());
            }
            Ok(())
        }
        // **The classification, printed from the binary that applies it.**
        //
        // `ci/ambient-falsify.py` reads `trace::FAMILIES` with a regex, which
        // is how every other structural gate in this tree reads Rust and is
        // also how every one of them can go stale against a refactor. This is
        // the same table read by the compiler instead, so the two can be
        // diffed on a tree where anything is in doubt — and it is what makes
        // `Family::tag` and `Family::reason` live fields rather than
        // documentation the linker deletes.
        Some("families") => {
            println!("kind\tclass\ttag\tarm\treason");
            for f in trace::FAMILIES {
                println!(
                    "{}\t{:?}\t{}\t{}\t{}",
                    f.kind,
                    f.class,
                    f.tag,
                    f.arm,
                    f.reason.split_whitespace().collect::<Vec<_>>().join(" ")
                );
            }
            Ok(())
        }
        // The other half of the same question, and a separate subcommand rather
        // than a flag because the two stamps have different lifetimes: the
        // projection belongs to a floor and the recorder belongs to a
        // recording. `docs/PARITY-ROADMAP.md` §15.4. `--explain` is shared and
        // prints both.
        Some("recorder-id") => {
            if args.iter().any(|a| a == "--explain") {
                print!("{}", compare::projection_explain());
            } else {
                println!("{}", record::recorder_identity());
            }
            Ok(())
        }
        _ => {
            eprintln!("usage:");
            eprintln!("  oracle-runner status  <host:port>");
            eprintln!("  oracle-runner join    <host:port> <name> [--stay-secs N]");
            eprintln!("  oracle-runner check   <host:port> <name> <checks.toml>");
            eprintln!(
                "  oracle-runner record  <host:port> <scenario-dir> --out <trace.json> \
                 [--console <console.fifo>]"
            );
            eprintln!(
                "  oracle-runner dump-recipe-displays <host:port> <candidates.json> \
                 --out <displays.json>"
            );
            eprintln!(
                "  oracle-runner compare <scenario-dir> --reference <a.json> \
                 --subject <b.json> [--baseline <b.toml>] [--json <out.json>]"
            );
            eprintln!(
                "  oracle-runner compare-serve <scenario-dir> --reference <a.json>   \
                 (stdin: <b.json>\t<out.json> per line; stdout: ok|err per line)"
            );
            eprintln!(
                "  oracle-runner explain <scenario-dir> --reference <a.json> \
                 --subject <b.json> [--only <name-substring>]"
            );
            eprintln!(
                "  oracle-runner project <scenario-dir> --trace <t.json> \
                 --json <out.json> [--lens declared|boundary-blind]"
            );
            eprintln!("  oracle-runner projection-id [--explain]");
            eprintln!("  oracle-runner recorder-id   [--explain]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn tables() -> io::Result<Tables> {
    let t = Tables::load(Path::new("data/vanilla"))?;
    // **Printed, not merely computed.** A coverage figure nothing reads is a
    // dead field behind a doc comment describing a gate no line implements —
    // the defect `docs/AGENT-BRIEF.md` opens with, and `-D warnings` catches
    // the first draft of it. `docs/PARITY-ROADMAP.md` §60 claims this number
    // exists, so it has to appear where a reader of a run can see it.
    //
    // The uncovered remainder is the honest statement of what a `sound`
    // observation still cannot grade: an unclassified event's pitch is
    // dropped, exactly as it was before 2026-08-17.
    let (known, total) = pitch::coverage();
    if total == 0 {
        eprintln!(
            "sound.pitch  registry absent — every pitch dropped (569323e behaviour); \
             data/vanilla/protocol_ids.json is missing"
        );
    } else {
        eprintln!("sound.pitch  {known}/{total} sound events classified, rest dropped");
    }
    Ok(t)
}

fn cmd_status(addr: &str) -> io::Result<()> {
    let t = Instant::now();
    let json = client::status(addr)?;
    println!(
        "STATUS OK  version={} protocol={} ping={}ms",
        json["version"]["name"].as_str().unwrap_or("?"),
        json["version"]["protocol"],
        t.elapsed().as_millis()
    );
    println!("{json}");
    Ok(())
}

fn cmd_join(addr: &str, name: &str, stay_secs: u64) -> io::Result<()> {
    let mut s = client::login_to_play(addr, name)?;
    println!(
        "JOIN OK  name={name} placed at ({:.1}, {:.1}, {:.1})",
        s.spawn[0], s.spawn[1], s.spawn[2]
    );
    s.drain(Duration::from_secs(stay_secs))?;
    println!("JOIN OK  survived {stay_secs}s in play state");
    // The chunk counts are the instrument that found the chunk-batch stall.
    // A conforming client sees one batch per acknowledgement and the whole
    // view distance arrives; a client that never acknowledges one sees a
    // single batch and then silence, and every broadcast about a chunk it
    // never received is withheld from it.
    println!(
        "JOIN OK  chunks={} batches={}",
        s.chunks_seen, s.batches_seen
    );
    Ok(())
}

/// The original scripted-chat check, kept because `ci/t2-wire.sh` is the
/// proof that this harness talks to **real vanilla** at all, and a harness
/// whose own liveness proof was deleted the day it grew is not one.
#[derive(serde::Deserialize)]
struct CheckFile {
    #[serde(rename = "check")]
    checks: Vec<Check>,
}

#[derive(serde::Deserialize)]
struct Check {
    name: String,
    send_command: String,
    expect_system_chat_contains: String,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

const fn default_timeout() -> u64 {
    5000
}

fn cmd_check(addr: &str, name: &str, file: &str) -> io::Result<()> {
    let parsed: CheckFile = toml::from_str(&std::fs::read_to_string(file)?)
        .map_err(|e| io::Error::other(format!("{file}: {e}")))?;
    let tables = tables()?;
    let mut s = client::login_to_play(addr, name)?;
    s.drain(Duration::from_millis(500))?;

    let (mut passed, mut failed) = (0u32, 0u32);
    for check in &parsed.checks {
        let step = scenario::Step::Command {
            text: check.send_command.clone(),
        };
        s.act(&step, &tables)?;
        let mut norm = trace::Normalizer::new(s.anchor);
        let mut obs = Vec::new();
        let started = Instant::now();
        s.pump(
            trace::Who::Actor,
            Instant::now() + Duration::from_millis(check.timeout_ms),
            &mut norm,
            &tables.states,
            &tables.item_names,
            0,
            started,
            &mut obs,
        )?;
        let hit = obs.iter().any(|o| {
            o.kind == "chat"
                && o.fields
                    .get("text")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t.contains(&check.expect_system_chat_contains))
        });
        if hit {
            passed += 1;
            println!("CHECK PASS  {}", check.name);
        } else {
            failed += 1;
            println!(
                "CHECK FAIL  {} (no chat containing {:?})",
                check.name, check.expect_system_chat_contains
            );
        }
    }
    println!(
        "CHECKS  passed={passed} failed={failed} total={}",
        passed + failed
    );
    if failed > 0 {
        return Err(io::Error::other(format!("{failed} check(s) failed")));
    }
    Ok(())
}

fn load_scenarios(path: &str) -> io::Result<Vec<Scenario>> {
    let p = Path::new(path);
    if p.is_dir() {
        Scenario::load_dir(p)
    } else {
        Ok(vec![Scenario::load(p)?])
    }
}

fn cmd_record(addr: &str, dir: &str, args: &[String]) -> io::Result<()> {
    let out = flag(args, "--out").ok_or_else(|| io::Error::other("record needs --out <file>"))?;
    let scenarios = load_scenarios(dir)?;
    if scenarios.is_empty() {
        return Err(io::Error::other(format!("no scenarios in {dir}")));
    }
    let tables = tables()?;
    // `--console <fifo>` is the reference server's stdin, and it is how a
    // scenario's `[[gamerule]]` reaches it. Only `ci/t2-gameplay.sh`'s vanilla
    // pass passes it; this server has no console and no game rules. See
    // `scenario::GameRule` for why that asymmetry is declared rather than
    // hidden, and `record::record` for the stderr line a scenario gets when it
    // asks for a rule on a target that cannot be told.
    let console = match flag(args, "--console") {
        Some(p) => Some(record::Console::open(p)?),
        None => None,
    };
    println!("recording {} scenario(s) against {addr}", scenarios.len());
    let trace = record::record(
        addr,
        &scenarios,
        &tables,
        concat!("oracle-runner ", env!("CARGO_PKG_VERSION")),
        console.as_ref(),
    )?;
    std::fs::write(out, serde_json::to_string_pretty(&trace)?)?;
    let failed = trace.scenarios.iter().filter(|s| s.error.is_some()).count();
    println!(
        "RECORDED  {} scenario(s), {failed} could not be run  ->  {out}",
        trace.scenarios.len()
    );
    for s in trace.scenarios.iter().filter(|s| s.error.is_some()) {
        println!("  !! {}: {}", s.name, s.error.clone().unwrap_or_default());
    }
    Ok(())
}

/// Capture real vanilla's `recipe_book_add` bytes, one item at a time.
///
/// `crate::recipe_book` (the server crate) replays these verbatim rather
/// than reconstructing `RecipeDisplayEntry`'s codec — see that module's doc
/// comment for why. This is where the bytes come from: hold an item, wait
/// for the packet, and if it is unambiguously **one** entry, keep it.
///
/// `candidates.json` is `{ "item id": ["recipe id", ...], ... }`, built by
/// `ci/dump-recipe-unlocks.sh` from vanilla's own advancement JSON. Only
/// candidates naming exactly one recipe are attempted: this dumper does not
/// parse `RecipeDisplayEntry`, so it has no way to split a multi-entry
/// packet, and a candidate this ambiguous is skipped rather than guessed at.
///
/// # Errors
/// If the target cannot be reached, `candidates.json` cannot be read, or the
/// output cannot be written.
fn cmd_dump_recipe_displays(addr: &str, candidates_path: &str, args: &[String]) -> io::Result<()> {
    let out = flag(args, "--out")
        .ok_or_else(|| io::Error::other("dump-recipe-displays needs --out <file>"))?;
    let tables = tables()?;
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(candidates_path)?)?;
    let candidates = doc
        .as_object()
        .ok_or_else(|| io::Error::other(format!("{candidates_path} is not a JSON object")))?;

    let mut s = client::login_to_play(addr, "recipedump")?;
    // The join burst — chunks, the initial inventory — has nothing to do
    // with a recipe book. Drained before the first hold so it cannot be
    // mistaken for one.
    s.drain(Duration::from_millis(500))?;

    let mut displays = serde_json::Map::new();
    let (mut got, mut silent, mut skipped) = (0usize, 0usize, 0usize);
    println!(
        "dumping recipe displays for {} candidate(s)",
        candidates.len()
    );
    for (item, recipes) in candidates {
        let recipe_ids: Vec<&str> = recipes
            .as_array()
            .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
            .unwrap_or_default();
        if recipe_ids.len() != 1 {
            skipped += 1;
            continue;
        }
        let recipe_id = recipe_ids[0];
        s.act(
            &scenario::Step::Hold {
                item: item.clone(),
                slot: 0,
                // A recipe-unlock probe asks what holding the *plain* item
                // unlocks. A component would change the stack this walk is
                // sweeping the whole registry with.
                components: Vec::new(),
            },
            &tables,
        )?;
        let until = Instant::now() + Duration::from_millis(1500);
        match s.capture(rustmc_proto::ids::play::cb::RECIPE_BOOK_ADD, until)? {
            None => silent += 1,
            Some(body) => match split_single_entry(&body) {
                Some(entry) => {
                    displays.insert(
                        recipe_id.to_string(),
                        serde_json::json!({"item": item, "entry_hex": to_hex(entry)}),
                    );
                    got += 1;
                }
                None => {
                    println!("  !! {item}: recipe_book_add was not exactly one entry, skipped");
                    skipped += 1;
                }
            },
        }
    }
    std::fs::write(
        out,
        serde_json::to_string_pretty(&serde_json::Value::Object(displays))?,
    )?;
    println!(
        "DUMPED  {got} recipe display(s), {silent} candidate(s) unlocked nothing, \
         {skipped} skipped (ambiguous)  ->  {out}"
    );
    Ok(())
}

/// The packet body is a `VarInt` entry count, then that many
/// `Entry`-codec blobs back to back with no length prefix of their own, then
/// one `replace` byte. Only `count == 1` is unambiguous without parsing the
/// entry itself — see [`cmd_dump_recipe_displays`].
fn split_single_entry(body: &[u8]) -> Option<&[u8]> {
    let (&count, rest) = body.split_first()?;
    if count != 1 {
        return None;
    }
    let (&replace, entry) = rest.split_last()?;
    (replace == 0 && !entry.is_empty()).then_some(entry)
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Print the projected observation sequences a comparison actually compared.
///
/// # Why this exists
///
/// `compare` prints `first_difference` for every non-`match`, which answers
/// "where did the two servers part company". It says nothing at all about a
/// check that **matched**, and on 2026-08-04 that was the question:
/// `140-fences-connect-to-each-other` was `match` while citing, in its own
/// `about`, the same uncarried connection-property gap that four diverging
/// checks cite. Either it is measuring something the others are not, or it is
/// a check that cannot fail — this project's oldest failure mode — and the
/// two are told apart by reading what the check observed, not by reading its
/// source.
///
/// The projection printed here is the one [`compare::compare`] compared,
/// carried out of it on the [`compare::Outcome`], never re-derived. A second
/// projection written for a debugging tool is a second instrument, and the
/// case where it disagrees with the first is exactly the case somebody would
/// be using it to investigate.
///
/// `inert` is the mechanised half of this question and it does not replace
/// it: `inert` asks whether the reference said anything the scenario's own
/// actions did not force, which is necessary and not sufficient. A check can
/// clear `informative` on an observation that carries no bearing on the
/// property the check is named for, and only reading the observations shows
/// that.
/// Dump ONE trace's per-scenario projection, for a stability measurement.
///
/// `ci/corpus-floor.py` reads N of these and asks how many distinct answers
/// the reference gave. That is not a pairwise question and cannot be got from
/// `compare`'s reports — see [`compare::Projected`] for why the difference
/// matters, and `ci/corpus-floor.py`'s header for what is done with it.
fn cmd_project(dir: &str, args: &[String]) -> io::Result<()> {
    let trace_path =
        flag(args, "--trace").ok_or_else(|| io::Error::other("project needs --trace <t.json>"))?;
    let out =
        flag(args, "--json").ok_or_else(|| io::Error::other("project needs --json <out.json>"))?;
    // Refused rather than defaulted. A mistyped lens that fell back to
    // `declared` would produce a boundary-blind report labelled declared, and
    // the whole point of the lens is that the label travels with the number.
    let lens = match flag(args, "--lens") {
        None => compare::Lens::Declared,
        Some(s) => compare::Lens::parse(s)
            .ok_or_else(|| io::Error::other(format!("unknown --lens {s}")))?,
    };
    let scenarios = load_scenarios(dir)?;
    let trace: trace::Trace = serde_json::from_str(&std::fs::read_to_string(trace_path)?)?;
    let projected = compare::project_trace(&scenarios, &trace, lens);
    let present = projected.iter().filter(|p| p.present).count();
    let doc = serde_json::json!({
        "lens": lens.name(),
        "trace": trace_path,
        "target": trace.target,
        "version": trace.version,
        "protocol": trace.protocol,
        "recorder": trace.recorder,
        "scenarios": projected,
    });
    std::fs::write(out, serde_json::to_string(&doc)?)?;
    println!(
        "PROJECTED  {present}/{} scenario(s) present in {trace_path}, lens {}  ->  {out}",
        scenarios.len(),
        lens.name()
    );
    Ok(())
}

fn cmd_explain(dir: &str, args: &[String]) -> io::Result<()> {
    let reference = flag(args, "--reference")
        .ok_or_else(|| io::Error::other("explain needs --reference <trace.json>"))?;
    let subject = flag(args, "--subject")
        .ok_or_else(|| io::Error::other("explain needs --subject <trace.json>"))?;
    let only = flag(args, "--only");
    // `--strict` prints the other denominator's projection instead of the
    // declared one. Without it the only way to find out why a strict verdict
    // diverged was to read two raw traces and re-apply the projection rules by
    // hand — a second instrument, in somebody's head, which is exactly the
    // arrangement this harness exists to refuse.
    let strict = args.iter().any(|a| a == "--strict");
    let scenarios = load_scenarios(dir)?;
    let reference: trace::Trace = serde_json::from_str(&std::fs::read_to_string(reference)?)?;
    let subject: trace::Trace = serde_json::from_str(&std::fs::read_to_string(subject)?)?;

    let outcomes = compare::compare(&scenarios, &reference, &subject);
    let mut shown = 0usize;
    for o in &outcomes {
        if only.is_some_and(|f| !o.name.contains(f)) {
            continue;
        }
        shown += 1;
        println!();
        println!(
            "== {}  declared {}  strict {}",
            o.name,
            o.verdict.name(),
            o.strict.name()
        );
        println!("   about: {}", o.about);
        let (rp, sp) = if strict {
            (&o.reference_strict_projection, &o.subject_strict_projection)
        } else {
            (&o.reference_projection, &o.subject_projection)
        };
        println!(
            "   {} observations: vanilla {}, ours {}",
            if strict { "strict" } else { "projected" },
            rp.len(),
            sp.len()
        );
        let n = rp.len().max(sp.len());
        for i in 0..n {
            let a = rp.get(i);
            let b = sp.get(i);
            let mark = if a == b { "  " } else { "->" };
            println!(
                "   {mark} [{i}] vanilla {}",
                a.map_or("(nothing)".into(), ToString::to_string)
            );
            println!(
                "      [{i}] ours    {}",
                b.map_or("(nothing)".into(), ToString::to_string)
            );
        }
        if !o.missing_kinds.is_empty() {
            println!("   we never sent: {}", o.missing_kinds.join(", "));
        }
        if !o.extra_kinds.is_empty() {
            println!("   we also sent:  {}", o.extra_kinds.join(", "));
        }
    }
    if shown == 0 {
        return Err(io::Error::other(format!(
            "no scenario matched {:?} — explain refuses to print nothing and call it an answer",
            only.unwrap_or("")
        )));
    }
    println!();
    println!("explained {shown} scenario(s)");
    Ok(())
}

/// The `--json` report. One function so `compare` and `compare-serve` cannot
/// write different documents for the same inputs.
fn write_report(
    path: &Path,
    reference: &trace::Trace,
    subject: &trace::Trace,
    outcomes: &[compare::Outcome],
) -> io::Result<()> {
    let doc = serde_json::json!({
        "reference": { "target": reference.target, "version": reference.version },
        "subject": { "target": subject.target, "version": subject.version },
        "declared": Counts::of(outcomes, false),
        "strict": Counts::of(outcomes, true),
        "outcomes": outcomes,
    });
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)
}

/// `compare --json`, many times, against one scenario directory and one
/// reference, each parsed once.
///
/// `ci/corpus-falsifiable.py --sweep` runs the comparator about 6,450 times
/// over one corpus and one reference, changing only the subject: on train-g's
/// 360-scenario pair, 63 ms of each 227 ms run was re-parsing the same 4 MB
/// reference and 20 ms re-reading the same 360 scenario files. Each stdin line
/// is `<subject.json>\t<out.json>`; the report written is `compare --json`'s,
/// by the same function, and one line goes back per request — `ok`, or `err`
/// and the reason — so a caller never reads a report this request did not
/// write. Nothing is gated here: the verdict lines, `--baseline` and the exit
/// status stay `compare`'s.
fn cmd_compare_serve(dir: &str, args: &[String]) -> io::Result<()> {
    use std::io::{BufRead, Write};
    let reference = flag(args, "--reference")
        .ok_or_else(|| io::Error::other("compare-serve needs --reference <trace.json>"))?;
    let scenarios = load_scenarios(dir)?;
    let reference: trace::Trace = serde_json::from_str(&std::fs::read_to_string(reference)?)?;
    let stdout = io::stdout();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let reply = (|| -> io::Result<()> {
            let (sub, out) = line
                .split_once('\t')
                .ok_or_else(|| io::Error::other("want <subject.json>\t<out.json>"))?;
            let subject: trace::Trace = serde_json::from_str(&std::fs::read_to_string(sub)?)?;
            let outcomes = compare::compare(&scenarios, &reference, &subject);
            write_report(Path::new(out), &reference, &subject, &outcomes)
        })();
        let mut o = stdout.lock();
        match reply {
            Ok(()) => writeln!(o, "ok")?,
            Err(e) => writeln!(o, "err {}", e.to_string().replace('\n', " "))?,
        }
        o.flush()?;
    }
    Ok(())
}

fn cmd_compare(dir: &str, args: &[String]) -> io::Result<()> {
    let reference = flag(args, "--reference")
        .ok_or_else(|| io::Error::other("compare needs --reference <trace.json>"))?;
    let subject = flag(args, "--subject")
        .ok_or_else(|| io::Error::other("compare needs --subject <trace.json>"))?;
    let scenarios = load_scenarios(dir)?;
    let reference: trace::Trace = serde_json::from_str(&std::fs::read_to_string(reference)?)?;
    let subject: trace::Trace = serde_json::from_str(&std::fs::read_to_string(subject)?)?;

    let outcomes = compare::compare(&scenarios, &reference, &subject);
    let declared = Counts::of(&outcomes, false);
    let strict = Counts::of(&outcomes, true);

    println!();
    println!(
        "reference: {} {} (protocol {})",
        reference.target, reference.version, reference.protocol
    );
    println!(
        "subject:   {} {} (protocol {})",
        subject.target, subject.version, subject.protocol
    );
    println!();
    for o in &outcomes {
        println!("  {:<8} {}", o.verdict.name(), o.name);
        if o.verdict != Verdict::Match {
            println!("           {}", o.about);
            if !o.missing_kinds.is_empty() {
                println!("           we never sent: {}", o.missing_kinds.join(", "));
            }
            if !o.extra_kinds.is_empty() {
                println!("           we also sent:  {}", o.extra_kinds.join(", "));
            }
            if let Some(d) = &o.first_difference {
                println!("           {d}");
            }
        } else if o.strict != Verdict::Match {
            // The whole reason two denominators are printed: this scenario
            // passed the thing it asked about and still said something
            // vanilla did not.
            println!("           (strict: {})", o.strict.name());
        }
    }

    println!();
    println!(
        "CORPUS  declared  {}/{} match   ({} diverge, {} absent, {} vacuous, {} mute, {} inert, \
         {} error)",
        declared.matched,
        declared.total,
        declared.diverged,
        declared.absent,
        declared.vacuous,
        declared.mute,
        declared.inert,
        declared.errored
    );
    println!(
        "CORPUS  strict    {}/{} match   ({} diverge, {} absent, {} vacuous, {} mute, {} inert, \
         {} error)",
        strict.matched,
        strict.total,
        strict.diverged,
        strict.absent,
        strict.vacuous,
        strict.mute,
        strict.inert,
        strict.errored
    );

    if let Some(path) = flag(args, "--json") {
        write_report(Path::new(path), &reference, &subject, &outcomes)?;
        println!("wrote {path}");
    }

    // A vacuous, mute, inert or errored scenario is always a fault, baseline
    // or not: none of the four says anything about the server, and a corpus
    // number that counts one is a number with a lie in its denominator.
    // `inert` is the newer of the two broken-check verdicts and the one that
    // needed inventing: since the unconditional resyncs were reproduced,
    // every placement scenario has a non-empty vanilla trace by construction,
    // so `vacuous` alone stopped being able to tell a live check from a dead
    // one.
    // Printed before the verdict lines, because it changes how every one of
    // them should be read. See `compare::session_path_divergence`: the
    // counter rebase repairs the two quantities we know are per-connection,
    // and this says whether there were others we do not know about.
    for line in &compare::session_path_divergence(&reference, &subject) {
        println!("SESSION-PATH  {line}");
    }

    // `mute` joins the three for the reason the other three are here: a
    // scenario whose measured step contributed nothing on either side says
    // nothing about the server, so counting it in the corpus number puts a
    // lie in the denominator. See `compare::Verdict::Mute`.
    let broken = declared.vacuous + declared.mute + declared.inert + declared.errored;
    let mut bad = Vec::new();
    if broken > 0 {
        for o in &outcomes {
            if !o.verdict.baselineable() {
                bad.push(format!(
                    "{}: {} — {}",
                    o.name,
                    o.verdict.name(),
                    o.first_difference
                        .as_deref()
                        .unwrap_or("vanilla itself produced nothing to compare against")
                ));
            }
        }
    }

    if let Some(path) = flag(args, "--baseline") {
        let baseline = Baseline::load(Path::new(path))?;
        let (worse, better) = baseline.check(&outcomes);
        for line in &better {
            println!("BETTER  {line}");
        }
        if !better.is_empty() {
            println!("        update {path} to lock these in");
        }
        bad.extend(worse);
    }

    if bad.is_empty() {
        println!("GAMEPLAY GREEN");
        Ok(())
    } else {
        for line in &bad {
            println!("REGRESSED  {line}");
        }
        Err(io::Error::other(format!(
            "{} scenario(s) unacceptable",
            bad.len()
        )))
    }
}
