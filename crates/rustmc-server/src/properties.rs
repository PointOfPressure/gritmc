//! `server.properties` — the operator settings this server reads.
//!
//! Vanilla loads the whole file through `DedicatedServerProperties`, which is
//! a `java.util.Properties` and roughly a hundred keys. This reads the two
//! that chunk streaming depends on and says nothing about the rest, because
//! a reader that silently defaulted a key it did not understand would be
//! indistinguishable from a server that had no such setting.
//!
//! # What was here before: nothing
//!
//! Until 2026-09-05 no code in this workspace opened `server.properties` at
//! all. `view-distance` arrived only as the `--view-distance` argv flag and
//! `simulation-distance` did not exist in any form: the join packet carried a
//! literal 10. An operator who wrote either key into the file they had been
//! told to write it into got no error and no effect.
//!
//! # Parsing
//!
//! `java.util.Properties.load` is a considerably larger grammar than this —
//! continuation lines, `\uXXXX` escapes, `:` as well as `=` — and none of it
//! is reproduced. What is reproduced is the part an operator uses: a `#` or
//! `!` comment line, a blank line, and `key=value` with the key up to the
//! first `=`. A line this cannot parse is **skipped and counted**, and the
//! count is returned, so a caller can say so rather than a malformed file
//! reading as an empty one.

use std::io;

/// The settings read out of `server.properties`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Properties {
    /// `view-distance`, if the file set it.
    pub view_distance: Option<i32>,
    /// `simulation-distance`, if the file set it.
    pub simulation_distance: Option<i32>,
    /// `entity-broadcast-range-percentage`, if the file set it —
    /// **already clamped**, unlike the two above.
    ///
    /// `DedicatedServerProperties.<init>` registers this key with a parse
    /// function rather than a plain `getInt`
    /// (`getMutable(String, Function, Integer)` at offsets 670-686), and that
    /// function is `lambda$new$1`:
    ///
    /// ```java
    /// Mth.clamp(Integer.parseInt(s), 10, 1000)
    /// ```
    ///
    /// So the clamp happens **when the file is read**, where
    /// `view-distance`'s happens where it is used
    /// (`ChunkMap.setServerViewDistance`). The difference is observable: an
    /// operator who writes `5` gets 10, and there is no later place that
    /// would have corrected it. The default is 100, and 100 is what makes
    /// `getScaledTrackingDistance` the identity.
    pub entity_broadcast_range_percentage: Option<i32>,
    /// Lines that were neither blank, a comment, nor `key=value`.
    ///
    /// Not an error: vanilla's `Properties.load` accepts a bare key as a key
    /// with an empty value, and refusing to start over a stray line would be
    /// a worse failure than ignoring it. Reported so the caller can print it.
    pub unparsed_lines: usize,
    /// Keys that parsed but whose value is not an integer, by name.
    ///
    /// Vanilla's `getInt` falls back to the default on a
    /// `NumberFormatException`, which is what this does — but it does it
    /// *loudly*, because a typo in `view-distance` that silently reverts to
    /// 10 is exactly the class of quiet default this project keeps paying
    /// for.
    pub bad_values: Vec<String>,
}

impl Properties {
    /// Parse the contents of a `server.properties`.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines() {
            let t = line.trim_start();
            if t.is_empty() || t.starts_with('#') || t.starts_with('!') {
                continue;
            }
            let Some((key, value)) = t.split_once('=') else {
                out.unparsed_lines += 1;
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            // `Mth.clamp(v, min, max)` is `min(max(v, min), max)`, so the
            // ceiling beats the floor. Only this key has one; see the field.
            let (slot, clamp) = match key {
                "view-distance" => (&mut out.view_distance, None),
                "simulation-distance" => (&mut out.simulation_distance, None),
                "entity-broadcast-range-percentage" => (
                    &mut out.entity_broadcast_range_percentage,
                    Some((10_i32, 1000_i32)),
                ),
                _ => continue,
            };
            match value.parse::<i32>() {
                Ok(v) => *slot = Some(clamp.map_or(v, |(lo, hi)| v.max(lo).min(hi))),
                Err(_) => out.bad_values.push(key.to_owned()),
            }
        }
        out
    }

    /// Read `server.properties` from a path.
    ///
    /// A missing file is `Ok(default)` and not an error: vanilla writes one
    /// on first start and runs on its built-in defaults until then. An
    /// unreadable file *is* an error — "not there" and "there and I cannot
    /// read it" are different facts and only one of them is normal.
    ///
    /// # Errors
    /// On any I/O failure other than the file not existing.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Properties;

    /// The shape of a real `server.properties` head, comments and all.
    #[test]
    fn the_two_keys_are_read_and_the_rest_ignored() {
        let p = Properties::parse(
            "#Minecraft server properties\n\
             #Fri Sep 05 12:00:00 BST 2026\n\
             \n\
             enable-jmx-monitoring=false\n\
             view-distance=4\n\
             max-players=20\n\
             simulation-distance=6\n\
             motd=A Minecraft Server\n",
        );
        assert_eq!(p.view_distance, Some(4));
        assert_eq!(p.simulation_distance, Some(6));
        assert_eq!(p.unparsed_lines, 0);
        assert!(p.bad_values.is_empty());
    }

    /// An absent key is `None` and not a default. The distinction is the
    /// whole point: the caller's default and "the operator wrote this" have
    /// to be separable, or an argv flag cannot override a file that is
    /// silent while losing to a file that is not.
    #[test]
    fn an_absent_key_is_none_rather_than_a_default() {
        let p = Properties::parse("motd=hi\n");
        assert_eq!(p.view_distance, None);
        assert_eq!(p.simulation_distance, None);
    }

    /// A typo is reported rather than silently defaulted.
    #[test]
    fn a_non_integer_value_is_named() {
        let p = Properties::parse("view-distance=lots\nsimulation-distance=6\n");
        assert_eq!(p.view_distance, None);
        assert_eq!(p.bad_values, vec!["view-distance".to_owned()]);
        assert_eq!(p.simulation_distance, Some(6));
    }

    /// A line with no `=` is counted, not fatal, and not confused with a key.
    #[test]
    fn a_malformed_line_is_counted() {
        let p = Properties::parse("view-distance\nview-distance=8\n");
        assert_eq!(p.unparsed_lines, 1);
        assert_eq!(p.view_distance, Some(8));
    }

    /// Negative and out-of-range values are read as written. Clamping is
    /// `ChunkMap.setServerViewDistance`'s job and doing it here would put the
    /// same clamp in two places.
    #[test]
    fn the_value_is_not_clamped_here() {
        let p = Properties::parse("view-distance=-3\nsimulation-distance=999\n");
        assert_eq!(p.view_distance, Some(-3));
        assert_eq!(p.simulation_distance, Some(999));
        assert_eq!(crate::tickets::server_view_distance(-3), 2);
        assert_eq!(crate::tickets::server_view_distance(999), 32);
    }

    /// `entity-broadcast-range-percentage` is clamped where it is parsed,
    /// which is the one key here that is — and both ends of the clamp are
    /// asserted, because a clamp with one live end is half a clamp.
    #[test]
    fn the_broadcast_percentage_is_clamped_at_parse_time() {
        for (written, want) in [(5, 10), (10, 10), (100, 100), (1000, 1000), (5000, 1000)] {
            let p = Properties::parse(&format!("entity-broadcast-range-percentage={written}\n"));
            assert_eq!(
                p.entity_broadcast_range_percentage,
                Some(want),
                "wrote {written}"
            );
        }
        // The control on the clamp being this key's and not everyone's: the
        // two distances go through the same `match` and come out untouched.
        let p = Properties::parse("view-distance=5000\nsimulation-distance=1\n");
        assert_eq!(p.view_distance, Some(5000));
        assert_eq!(p.simulation_distance, Some(1));
    }

    /// A missing file is not an error.
    #[test]
    fn a_missing_file_reads_as_empty() {
        let p = Properties::load(std::path::Path::new(
            "/nonexistent/rustmc/server.properties",
        ))
        .expect("a missing file is not an error");
        assert_eq!(p, Properties::default());
    }
}
