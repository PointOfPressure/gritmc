//! goldengen — regenerates golden vectors from the **vanilla jar** (ORACLE.md
//! §3–5).
//!
//! Compiles and runs `vanilla/GoldenDump.java` against the pinned server jar,
//! writing JSON straight into a crate's `golden/` directory. Regeneration is
//! deliberate — a version bump, or a vector shown to be wrong — never part of
//! a normal build. Nothing here runs in CI; the committed vectors are what
//! the gates read.
//!
//! Ground truth is the jar, not another implementation. The first run of this
//! tool replaced vectors exported from Minecom, two of which were wrong:
//! Minecom's `Mth.getSeed` widens `x` to 64 bits before multiplying where
//! vanilla multiplies in 32 (`javap -c net.minecraft.util.Mth` shows
//! `ldc 3129871; imul; i2l`), so its positional seeds diverge for
//! |x| > ~686 blocks — far enough from spawn that a spawn-centred region diff
//! never sees it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if let Err(message) = run() {
        eprintln!("goldengen: {message}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "usage: goldengen [<outdir>]\n\
             \n\
             Regenerates RNG golden vectors from the vanilla server jar.\n\
             Default outdir: crates/rustmc-worldgen/golden\n\
             \n\
             Environment:\n  \
               RUSTMC_VANILLA_JAR   path to the server jar\n  \
               RUSTMC_VANILLA_LIBS  directory tree of vanilla's library jars"
        );
        return Ok(());
    }

    let repo = repo_root();
    let out_dir = args
        .first()
        .map_or_else(|| repo.join("crates/rustmc-worldgen/golden"), PathBuf::from);

    let jar = locate_jar(&repo)?;
    let libs = locate_libs(&jar)?;
    eprintln!("goldengen: jar  {}", jar.display());
    eprintln!("goldengen: libs {} jar(s)", libs.len());

    // Vanilla's own classes static-init against JOML and Guava, so the full
    // library set has to be on the classpath even for a pure-math dump.
    let mut classpath = vec![jar.display().to_string()];
    classpath.extend(libs.iter().map(|p| p.display().to_string()));
    let classpath = classpath.join(":");

    let source = repo.join("tools/goldengen/vanilla/GoldenDump.java");
    if !source.is_file() {
        return Err(format!("dump source missing: {}", source.display()));
    }

    let build_dir = repo.join("oracle-work/goldengen-classes");
    std::fs::create_dir_all(&build_dir)
        .map_err(|e| format!("mkdir {}: {e}", build_dir.display()))?;

    run_tool(
        Command::new("javac")
            .arg("-nowarn")
            .args(["-cp", &classpath])
            .arg("-d")
            .arg(&build_dir)
            .arg(&source),
        "javac",
    )?;

    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir {}: {e}", out_dir.display()))?;
    run_tool(
        Command::new("java")
            .args(["-cp", &format!("{}:{classpath}", build_dir.display())])
            .arg("GoldenDump")
            .arg(&out_dir),
        "java",
    )?;

    eprintln!(
        "goldengen: regenerated vectors in {} — review the diff before committing",
        out_dir.display()
    );
    Ok(())
}

fn run_tool(command: &mut Command, name: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("could not run {name}: {e} (is a JDK on PATH?)"))?;
    if !status.success() {
        return Err(format!("{name} failed with {status}"));
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    // tools/goldengen/ -> repo root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the repo root")
        .to_path_buf()
}

fn locate_jar(repo: &Path) -> Result<PathBuf, String> {
    if let Ok(explicit) = std::env::var("RUSTMC_VANILLA_JAR") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "RUSTMC_VANILLA_JAR is not a file: {}",
            path.display()
        ));
    }

    // The version ci/vanilla-server.sh pins, so both read the same jar.
    let version = game_version(repo)?;

    // What `ci/vanilla-server.sh fetch` downloads is a *bundler* jar: it
    // holds only the launcher shim at the top level and carries the real
    // classes in a nested archive. Compiling against it fails with "cannot
    // find symbol" on every `net.minecraft` import. Both candidates below are
    // therefore the extracted inner jar, which appears once the server has
    // been started (or unpacked) once.
    let candidates = [
        repo.join(format!(
            "oracle-work/vanilla-{version}/versions/{version}/server-{version}.jar"
        )),
        home()
            .join(format!("mc-{version}"))
            .join("versions")
            .join(&version)
            .join(format!("server-{version}.jar")),
    ];

    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        return Ok(found.clone());
    }

    let bundler = repo.join(format!("oracle-work/vanilla-{version}/server.jar"));
    let hint = if bundler.is_file() {
        format!(
            "\nThe bundler jar at {} is present but not usable directly — run \
             `ci/vanilla-server.sh start` once to unpack it.",
            bundler.display()
        )
    } else {
        "\nRun `ci/vanilla-server.sh fetch`, or set RUSTMC_VANILLA_JAR.".to_string()
    };

    Err(format!(
        "no usable vanilla {version} jar found. Tried:\n  {}{hint}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    ))
}

fn locate_libs(jar: &Path) -> Result<Vec<PathBuf>, String> {
    if let Ok(explicit) = std::env::var("RUSTMC_VANILLA_LIBS") {
        return Ok(collect_jars(Path::new(&explicit)));
    }

    // A launcher-style install keeps `libraries/` beside `versions/`.
    let launcher_libs = jar
        .ancestors()
        .nth(3)
        .map(|root| root.join("libraries"))
        .filter(|p| p.is_dir());
    if let Some(dir) = launcher_libs {
        let jars = collect_jars(&dir);
        if !jars.is_empty() {
            return Ok(jars);
        }
    }

    // A bundler-style server jar carries its libraries inside; the extracted
    // form lands next to the jar once the server has been started once.
    let sibling = jar.parent().map(|d| d.join("libraries"));
    if let Some(dir) = sibling.filter(|p| p.is_dir()) {
        let jars = collect_jars(&dir);
        if !jars.is_empty() {
            return Ok(jars);
        }
    }

    Err(
        "could not find vanilla's library jars (JOML/Guava are needed for \
         static init). Set RUSTMC_VANILLA_LIBS to a directory tree of jars."
            .to_string(),
    )
}

fn collect_jars(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "jar") {
                found.push(path);
            }
        }
    }
    // Deterministic classpath order — a shifting classpath is a debugging tax
    // nobody needs.
    found.sort();
    found
}

fn game_version(repo: &Path) -> Result<String, String> {
    let lib = repo.join("crates/rustmc-proto/src/lib.rs");
    let text = std::fs::read_to_string(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
    text.split("GAME_VERSION: &str = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .ok_or_else(|| format!("GAME_VERSION not found in {}", lib.display()))
}

fn home() -> PathBuf {
    std::env::var("HOME").map_or_else(|_| PathBuf::from("."), PathBuf::from)
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// `goldengen` writes the golden vectors 42 T1 targets are adjudicated
/// against. Everything it does needs a jar and a JVM, so nothing here can
/// gate the dump itself — but the two pure decisions it makes *before* the
/// JVM starts are the ones that silently pick the wrong ground truth, and
/// neither had ever been executed by a test.
#[cfg(test)]
mod tests {
    use super::{collect_jars, game_version, repo_root};
    use std::path::PathBuf;

    /// **The scrape and the compiler must agree about which version this is.**
    ///
    /// `game_version` finds `GAME_VERSION: &str = "` by string split, so it is
    /// coupled to the *formatting* of a line in another crate. Reflow that
    /// declaration — a `const` on two lines, a different quote style, a
    /// `concat!` — and this function silently fails or, worse, finds an older
    /// literal somewhere else in the file, and goldengen compiles the dumper
    /// against the wrong jar while reporting success.
    ///
    /// The comparison is against `rustmc_proto::GAME_VERSION` rather than
    /// against a literal here, because a literal here would be a third copy to
    /// keep in step and would pass while both it and the scrape were wrong.
    #[test]
    fn the_scraped_version_is_the_one_the_compiler_sees() {
        assert_eq!(
            game_version(&repo_root()).expect("GAME_VERSION must be scrapable from the tree"),
            rustmc_proto::GAME_VERSION
        );
    }

    /// And the failure has to be a refusal. A `game_version` that answered
    /// `Ok("")` on an unparseable file would build a jar path with an empty
    /// version in it and report "no usable vanilla  jar found".
    #[test]
    fn a_tree_with_no_version_constant_is_refused() {
        let dir = tmpdir("goldengen-noversion");
        let src = dir.join("crates/rustmc-proto/src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "pub const SOMETHING_ELSE: &str = \"26.2\";\n",
        )
        .unwrap();
        let got = game_version(&dir);
        assert!(
            got.is_err(),
            "a source with no GAME_VERSION must refuse, got {got:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// "Deterministic classpath order — a shifting classpath is a debugging
    /// tax nobody needs", says the function. Nothing checked it, and
    /// `read_dir` returns entries in whatever order the filesystem holds them.
    #[test]
    fn the_classpath_is_sorted_and_holds_only_jars() {
        let dir = tmpdir("goldengen-jars");
        std::fs::create_dir_all(dir.join("nested/deep")).unwrap();
        for path in [
            "zeta.jar",
            "alpha.jar",
            "notes.txt",
            "nested/mid.jar",
            "nested/deep/inner.jar",
            "nested/deep/README",
        ] {
            std::fs::write(dir.join(path), b"x").unwrap();
        }
        let got = collect_jars(&dir);
        let names: Vec<String> = got
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["alpha.jar", "inner.jar", "mid.jar", "zeta.jar"]);
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(got, sorted, "the classpath must be in sorted path order");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A directory that is not there is not a classpath of one jar, and it is
    /// not a panic either — `locate_libs` relies on the empty answer to fall
    /// through to its next candidate.
    #[test]
    fn a_missing_directory_yields_no_jars_rather_than_failing() {
        assert!(collect_jars(&PathBuf::from("/nonexistent/goldengen/libs")).is_empty());
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
