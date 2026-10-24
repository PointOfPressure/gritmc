//! The scoreboard's five packets and five enum tables against vanilla's own
//! bytes.
//!
//! `crates/rustmc-server/golden/scoreboard.json` is produced by
//! `ci/scoreboard-golden.sh`: it builds real `Objective` and `PlayerTeam`
//! objects inside the pinned jar, encodes eighteen packets with their own
//! `STREAM_CODEC`, and dumps the wire id of every enum constant those packets
//! carry.
//!
//! # What can turn this red
//!
//! Every field order, the plain-byte method fields (a varint would agree on
//! all eighteen and disagree on the first method above 127), the
//! non-zigzag varint score, `reset_score`'s nullable objective, the 26.2
//! `Parameters` order, the `options` bit assignment, the network-NBT
//! component form, and any renumbering of the five enum tables.
//!
//! # What cannot
//!
//! Number formats. Every capture has `Optional.empty()` there, so the one
//! byte written is `false` and no format payload is exercised at all —
//! `set_objective` says so where it writes it. Styled components likewise:
//! every display name here is a plain literal.

use std::path::PathBuf;

use rustmc_server::scoreboard::{
    literal, packets, team_method, PlayerTeam, Scoreboard, COLLISION_RULES, DISPLAY_SLOTS,
    METHOD_ADD, METHOD_CHANGE, METHOD_REMOVE, RENDER_TYPES, TEAM_COLORS, VISIBILITIES,
};

fn golden() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/scoreboard.json");
    assert!(
        rustmc_data::fixture::available(&p, "scoreboard golden", "ci/scoreboard-golden.sh"),
        "no scoreboard golden"
    );
    serde_json::from_slice(&std::fs::read(&p).expect("read golden")).expect("parse golden")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Every constant in this module is the number vanilla writes.
///
/// This is the check that makes the `id`-versus-ordinal distinction in the
/// module docs load-bearing rather than decorative: if a future version
/// renumbers one of these, the table below stops matching and the packets
/// that carry it are wrong before anyone reads the wire.
#[test]
fn enum_tables_match_vanilla() {
    let g = golden();
    let mut n = 0usize;
    for (key, table) in [
        ("displaySlots", &DISPLAY_SLOTS[..]),
        ("teamColors", &TEAM_COLORS[..]),
        ("visibility", &VISIBILITIES[..]),
        ("collisionRules", &COLLISION_RULES[..]),
        ("renderTypes", &RENDER_TYPES[..]),
    ] {
        let v = g[key].as_object().unwrap_or_else(|| panic!("no {key}"));
        assert_eq!(
            v.len(),
            table.len(),
            "{key}: vanilla has {} entries, we declare {}",
            v.len(),
            table.len()
        );
        for (name, id) in table {
            let want = v
                .get(*name)
                .unwrap_or_else(|| panic!("{key}: vanilla has no {name}"))
                .as_i64()
                .unwrap();
            assert_eq!(i64::from(*id), want, "{key}.{name}");
            n += 1;
        }
    }
    assert_eq!(n, 45, "45 enum constants across five tables");
}

/// The eighteen captured packets, byte for byte.
#[test]
fn every_packet_matches_vanilla() {
    let g = golden();
    let p = &g["packets"];
    let want = |k: &str| {
        p[k].as_str()
            .unwrap_or_else(|| panic!("no captured {k}"))
            .to_string()
    };

    assert_eq!(i64::from(METHOD_ADD), g["methodAdd"].as_i64().unwrap());
    assert_eq!(
        i64::from(METHOD_REMOVE),
        g["methodRemove"].as_i64().unwrap()
    );
    assert_eq!(
        i64::from(METHOD_CHANGE),
        g["methodChange"].as_i64().unwrap()
    );

    // The two objectives the Java script builds.
    let mut kills = packets::plain_objective("kills", "dummy", "integer");
    kills.display_name = literal("Kills");
    let mut hp = packets::plain_objective("hp", "health", "hearts");
    hp.display_name = literal("HP");
    hp.display_auto_update = true;

    let enc = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut b = Vec::new();
        f(&mut b);
        hex(&b)
    };

    let mut n = 0usize;
    for (key, got) in [
        (
            "set_objective/add/kills",
            enc(&|b| packets::set_objective(b, &kills, METHOD_ADD).unwrap()),
        ),
        (
            "set_objective/change/kills",
            enc(&|b| packets::set_objective(b, &kills, METHOD_CHANGE).unwrap()),
        ),
        (
            "set_objective/remove/kills",
            enc(&|b| packets::set_objective(b, &kills, METHOD_REMOVE).unwrap()),
        ),
        (
            "set_objective/add/hp-hearts",
            enc(&|b| packets::set_objective(b, &hp, METHOD_ADD).unwrap()),
        ),
        (
            "set_score/positive",
            enc(&|b| packets::set_score(b, "oracle", "kills", 7, None).unwrap()),
        ),
        (
            "set_score/negative",
            enc(&|b| packets::set_score(b, "oracle", "kills", -3, None).unwrap()),
        ),
        (
            "set_score/with-display",
            enc(&|b| {
                packets::set_score(b, "oracle", "kills", 0, Some(&literal("seven"))).unwrap();
            }),
        ),
        (
            "reset_score/one-objective",
            enc(&|b| packets::reset_score(b, "oracle", Some("kills")).unwrap()),
        ),
        (
            "reset_score/all-objectives",
            enc(&|b| packets::reset_score(b, "oracle", None).unwrap()),
        ),
        (
            "set_display_objective/list",
            enc(&|b| packets::set_display_objective(b, "list", "kills").unwrap()),
        ),
        (
            "set_display_objective/sidebar",
            enc(&|b| packets::set_display_objective(b, "sidebar", "kills").unwrap()),
        ),
        (
            "set_display_objective/sidebar.team.white",
            enc(&|b| packets::set_display_objective(b, "sidebar.team.white", "kills").unwrap()),
        ),
    ] {
        assert_eq!(got, want(key), "{key}");
        n += 1;
    }

    // The team the Java script builds, assembled through the container so the
    // index-keeping in `add_player_to_team` is on the path rather than beside
    // it.
    let mut sb = Scoreboard::default();
    let mut reds = PlayerTeam::new("reds");
    reds.display_name = literal("The Reds");
    reds.player_prefix = literal("[");
    reds.player_suffix = literal("]");
    reds.color = Some("red".into());
    reds.allow_friendly_fire = false;
    reds.see_friendly_invisibles = true;
    reds.name_tag_visibility = "hideForOtherTeams".into();
    reds.collision_rule = "pushOwnTeam".into();
    sb.add_team(reds).unwrap();
    assert!(sb.add_player_to_team("oracle", "reds"));
    assert!(sb.add_player_to_team("subject", "reds"));
    let reds = sb.team("reds").unwrap().clone();
    assert_eq!(reds.players, ["oracle", "subject"]);
    assert_eq!(reds.options(), 2, "friendly fire off, see invisibles on");

    let one = vec!["oracle".to_string()];
    for (key, got) in [
        (
            "set_player_team/create",
            enc(&|b| {
                packets::set_player_team(b, &reds, team_method::CREATE, &reds.players).unwrap();
            }),
        ),
        (
            "set_player_team/update",
            enc(&|b| {
                packets::set_player_team(b, &reds, team_method::UPDATE, &reds.players).unwrap();
            }),
        ),
        (
            "set_player_team/remove",
            enc(&|b| packets::set_player_team(b, &reds, team_method::REMOVE, &[]).unwrap()),
        ),
        (
            "set_player_team/join",
            enc(&|b| packets::set_player_team(b, &reds, team_method::JOIN, &one).unwrap()),
        ),
        (
            "set_player_team/leave",
            enc(&|b| packets::set_player_team(b, &reds, team_method::LEAVE, &one).unwrap()),
        ),
    ] {
        assert_eq!(got, want(key), "{key}");
        n += 1;
    }

    // The second team: no colour, and `seeFriendlyInvisibles` turned off, so
    // both the options byte and the colour's presence bit take their other
    // value somewhere in the capture. A fresh team is 3 (both flags start
    // true, `PlayerTeam.<init>` offsets 26 and 31); this one is 1.
    let mut greys = PlayerTeam::new("greys");
    assert_eq!(greys.options(), 3, "a fresh team is 3, not 0");
    greys.allow_friendly_fire = true;
    greys.see_friendly_invisibles = false;
    assert_eq!(greys.options(), 1);
    assert_eq!(
        enc(&|b| packets::set_player_team(b, &greys, team_method::CREATE, &[]).unwrap()),
        want("set_player_team/create-defaults")
    );
    n += 1;

    assert_eq!(n, 18, "eighteen captures compared");
}

/// Vanilla's bytes decode back, for the two packets this crate can decode
/// without a component or a registry.
#[test]
fn captured_packets_decode() {
    let g = golden();
    let raw = |k: &str| -> Vec<u8> {
        let s = g["packets"][k].as_str().unwrap();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    };

    let b = raw("reset_score/one-objective");
    let (owner, obj) = packets::read_reset_score(&mut &b[..]).unwrap();
    assert_eq!((owner.as_str(), obj.as_deref()), ("oracle", Some("kills")));
    let b = raw("reset_score/all-objectives");
    let (owner, obj) = packets::read_reset_score(&mut &b[..]).unwrap();
    assert_eq!((owner.as_str(), obj), ("oracle".to_string().as_str(), None));

    let b = raw("set_display_objective/sidebar.team.white");
    let (slot, obj) = packets::read_set_display_objective(&mut &b[..]).unwrap();
    assert_eq!(
        (slot.as_str(), obj.as_str()),
        ("sidebar.team.white", "kills")
    );
}

/// Four ways to be wrong, each shown to move a captured byte.
#[test]
fn positive_control() {
    let g = golden();
    let p = &g["packets"];

    // 1. The score varint is not zigzag. Zigzag(-3) is one byte; the capture
    //    is five.
    let neg = p["set_score/negative"].as_str().unwrap();
    let pos = p["set_score/positive"].as_str().unwrap();
    assert_eq!(
        neg.len() - pos.len(),
        8,
        "a negative score should be four bytes longer, not {}",
        (neg.len() - pos.len()) / 2
    );

    // 2. `options` is not a constant. If it were, the two team captures would
    //    agree on that byte and this check would be inert.
    assert_ne!(
        p["set_player_team/create"].as_str().unwrap(),
        p["set_player_team/create-defaults"].as_str().unwrap()
    );
    assert_eq!(PlayerTeam::new("x").options(), 3);
    let mut t = PlayerTeam::new("x");
    t.allow_friendly_fire = false;
    assert_eq!(t.options(), 2);
    t.see_friendly_invisibles = false;
    assert_eq!(t.options(), 0);

    // 3. `REMOVE` really does stop early in both packets that have one.
    assert!(
        p["set_objective/remove/kills"].as_str().unwrap().len()
            < p["set_objective/add/kills"].as_str().unwrap().len()
    );
    assert!(
        p["set_player_team/remove"].as_str().unwrap().len()
            < p["set_player_team/create"].as_str().unwrap().len()
    );

    // 4. The container's index really removes a player from their old team.
    //    Without the `remove_player_from_team` inside `add_player_to_team`,
    //    `reds` would still list them and `set_player_team/create` would carry
    //    a name that is on two teams.
    let mut sb = Scoreboard::default();
    sb.add_team(PlayerTeam::new("reds")).unwrap();
    sb.add_team(PlayerTeam::new("blues")).unwrap();
    assert!(sb.add_player_to_team("oracle", "reds"));
    assert!(sb.add_player_to_team("oracle", "blues"));
    assert_eq!(sb.team("reds").unwrap().players.len(), 0);
    assert_eq!(sb.team("blues").unwrap().players, ["oracle"]);
    assert_eq!(sb.team_of_player("oracle").unwrap().name, "blues");
    // And the second add of the same pair is a no-op, not a duplicate.
    assert!(!sb.add_player_to_team("oracle", "blues"));
    assert_eq!(sb.team("blues").unwrap().players, ["oracle"]);
}
