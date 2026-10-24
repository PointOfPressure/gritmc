#!/usr/bin/env python3
"""The mutations `ci/plugin-hook-falsify.sh` applies, one at a time.

Each entry breaks or subverts exactly one link in the chain from a dig packet
to a plugin's refusal, and the sweep requires every one of them to turn the
suite red. A mutation that stays green is a link nothing is checking, and the
sweep prints it as UNCAUGHT rather than passing quietly.

`expect` is what the sweep predicts, and it is written down **before** the run
rather than after it, so a surprise is visible as a surprise. `red` means the
suite must fail; `green` means the sweep predicts the mutation is invisible to
this crate's tests and says why — a prediction that is itself falsifiable,
because a `green` entry that comes back red means the reasoning was wrong.

Usage:
    plugin-hook-mutate.py --list
    plugin-hook-mutate.py --snapshot      # once, before any --apply
    plugin-hook-mutate.py --apply <id>
    plugin-hook-mutate.py --revert
"""

import json
import os
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WS = "crates/rustmc-server/src/world_send.rs"
PL = "crates/rustmc-server/src/plugins.rs"
ST = "crates/rustmc-server/src/status.rs"
BC = "crates/rustmc-server/src/broadcast.rs"

BACKUP = os.path.join(ROOT, ".plugin-hook-falsify-backup")

# (id, file, old, new, expect, why)
MUTATIONS = [
    (
        "M01-break-hook-deleted",
        WS,
        "if crate::plugins::block_break_denied(s, action.pos).is_none() {",
        "if true {",
        "red",
        "the hook is not called at all: the server is back to where it was.",
    ),
    (
        "M02-break-verdict-ignored",
        WS,
        "if crate::plugins::block_break_denied(s, action.pos).is_none() {",
        "if {\n                let _ = crate::plugins::block_break_denied(s, action.pos);\n                true\n            } {",
        "red",
        "the hook IS called and its answer is thrown away. This is the one "
        "that separates 'a plugin was asked' from 'a plugin was obeyed'.",
    ),
    (
        "M03-break-hook-inert",
        PL,
        "    let rt = s.plugins?;\n    if rt.breaks == 0 {\n        return None;\n    }\n    let host = Host::for_session(s);",
        "    let rt = s.plugins?;\n    if rt.breaks == 0 {\n        return None;\n    }\n    return None;\n    #[allow(unreachable_code)]\n    let host = Host::for_session(s);",
        "red",
        "the call site is intact and the implementation always allows. A test "
        "that only checked the hook was reached would pass here.",
    ),
    (
        "M04-break-hook-always-denies",
        PL,
        "    let reason = rt.deny_reason(&event, &mut ctx)?;\n    ctx.message(player, &reason);\n    Some(reason)\n}\n\n/// Ask the plugins whether `player` may place",
        "    let _ = rt.deny_reason(&event, &mut ctx);\n    let reason = \"no\".to_owned();\n    ctx.message(player, &reason);\n    Some(reason)\n}\n\n/// Ask the plugins whether `player` may place",
        "red",
        "everything is refused. Only the half of the slice that digs OUTSIDE "
        "the claim can see this — a one-sided test would call it a pass.",
    ),
    (
        "M05-zero-plugin-gate-inverted",
        PL,
        "    if rt.breaks == 0 {\n        return None;\n    }",
        "    if rt.breaks != 0 {\n        return None;\n    }",
        "red",
        "the fast path for a server with no plugins now swallows the case "
        "where there ARE plugins.",
    ),
    (
        "M06-denial-message-dropped",
        PL,
        "    let reason = rt.deny_reason(&event, &mut ctx)?;\n    ctx.message(player, &reason);\n    Some(reason)\n}\n\n/// Ask the plugins whether `player` may place",
        "    let reason = rt.deny_reason(&event, &mut ctx)?;\n    Some(reason)\n}\n\n/// Ask the plugins whether `player` may place",
        "red",
        "the block is protected and the player is told nothing, which is "
        "indistinguishable from lag.",
    ),
    (
        "M07-event-carries-the-wrong-state",
        PL,
        "    let host = Host::for_session(s);\n    let state = host.handle_at(pos)?;",
        "    let host = Host::for_session(s);\n    let state = interner().intern(\"minecraft:air\");",
        "red",
        "the event names the air that replaces the block rather than the block "
        "being removed, so a handler branching on state branches on the wrong "
        "one.",
    ),
    (
        "M08-place-hook-deleted",
        WS,
        "                let denied = state\n                    .as_deref()\n                    .is_some_and(|st| crate::plugins::block_place_denied(s, target, st).is_some());",
        "                let denied = false;",
        "red",
        "breaking is protected and placing is not, so anyone can wall a claim "
        "in — and `grit:event.block_place` is published with no call path.",
    ),
    (
        "M09-interner-collapsed",
        PL,
        "        let leaked: &'static str = Box::leak(state.to_owned().into_boxed_str());",
        "        let leaked: &'static str = \"minecraft:air\";",
        "red",
        "every state interns to the same text, so every handle is a lie and "
        "`state_text` answers for a block that is not there.",
    ),
    (
        "M10-capability-overpublished",
        PL,
        "            cap::EVENT_BLOCK_PLACE,\n        ])\n        .expect(\"literal capabilities\")",
        "            cap::EVENT_BLOCK_PLACE,\n            cap::COMMAND_REGISTER,\n        ])\n        .expect(\"literal capabilities\")",
        "red",
        "the host claims a capability with no production call path. This is "
        "the exact failure capability negotiation exists to prevent, and the "
        "plugin that requires it would load and then never be called.",
    ),
    (
        "M11-host-write-is-a-noop",
        PL,
        "        let mut updates = Vec::new();\n        let wrote = crate::shapes::set_block(",
        "        let mut updates = Vec::new();\n        if true {\n            return SetOutcome::Changed;\n        }\n        #[allow(unreachable_code)]\n        let wrote = crate::shapes::set_block(",
        "red",
        "a plugin's own write reports success and touches nothing. Every test "
        "that only asserts the server did NOT write stays green here.",
    ),
    (
        "M12-plugin-writes-are-never-broadcast",
        PL,
        "        if let Some(changed) = self.changed {\n            for &(p, _) in &updates {\n                changed.record(p);\n            }\n        }",
        "",
        "red",
        "a plugin's edits land in the world and no client is ever told. "
        "**This came back green on the first sweep**: the assertion was "
        "'some block_update was sent', and the dig's own removal had already "
        "put one on the wire. The handler now writes 18 blocks up — a "
        "different 16-block section, so a different packet — and the "
        "assertion names the body rather than the id.",
    ),
    (
        "M13-tick-lock-not-held-across-the-dig",
        WS,
        "fn handle_player_action(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {\n    use crate::interact::{decode_player_action, Action};\n\n    // Held for the whole handler: vanilla's game loop is single-threaded, so\n    // a tick's broadcasts can never land between a handler's world edit and\n    // the acknowledgement it owes. See `crate::broadcast::tick_lock`.\n    let _tick = crate::broadcast::tick_lock();",
        "fn handle_player_action(s: &mut Session<'_>, body: &[u8]) -> io::Result<()> {\n    use crate::interact::{decode_player_action, Action};\n",
        "red",
        "the contract promises a plugin author exactly one callback at any "
        "instant server-wide, and this lock is the whole of how that promise "
        "is kept. **This was caught on the first sweep and MISSED on the "
        "second**, from an unchanged mutation: the observer was a `--lib` "
        "test, the tick lock is process-wide, and another test's handler was "
        "supplying the `true`. The observer now lives alone in "
        "`tests/plugin_tick_lock.rs` and opens by asserting nothing holds the "
        "lock, so a `true` can only have come from the call it made itself.",
    ),
    (
        "M14-loader-bypassed",
        PL,
        "        let report: LoadReport = load(plugins, &host, &mut bus);",
        "        drop(plugins);\n        let report: LoadReport = LoadReport::default();",
        "red",
        "the real loader — sorted by plugin id, API version checked, "
        "capabilities checked by name — is skipped and nothing is enabled.",
    ),
    (
        "M15-production-session-unwired",
        ST,
        "                plugins: crate::plugins::global(),",
        "                plugins: None,",
        "red",
        "the SHIPPED server is silently plugin-less. No behavioural test in "
        "this crate can reach `status::listen`, so this is caught by the "
        "source-level guard in `plugins::tests` and by nothing else — which "
        "is the point of that guard existing.",
    ),
    (
        "M16-plugin-under-declares-its-capabilities",
        WS,
        "                requires: CapabilitySet::from_strs([\n                    cap::EVENT_BLOCK_BREAK,\n                    cap::EVENT_BLOCK_PLACE,\n                ])\n                .expect(\"literals\"),",
        "                requires: CapabilitySet::from_strs([cap::EVENT_BLOCK_BREAK])\n                    .expect(\"literals\"),",
        "green",
        "PREDICTED UNCAUGHT, and predicted deliberately. A manifest is a "
        "declaration of what a plugin cannot work WITHOUT; it only bites "
        "against a host that lacks the capability. This server publishes "
        "block_place, so an under-declaring plugin still loads and still "
        "works. Catching it needs a host missing the capability, which is "
        "`grit-plugins`' own test to own, not this repo's. A `red` here "
        "would mean the server had grown a check the contract does not "
        "describe.",
    ),
    (
        "M17-tick-lock-probe-cannot-fail",
        BC,
        "    matches!(m.try_lock(), Err(std::sync::TryLockError::WouldBlock))",
        "    true",
        "red",
        "the instrument for M13, checked against itself. **This came back "
        "green on the first sweep** — the end-to-end test only ever asks the "
        "probe to report a lock that IS held, so a probe hardwired to `true` "
        "satisfied it perfectly. `broadcast::is_held` now takes the mutex as "
        "an argument and `the_lock_probe_reports_free_as_well_as_held` "
        "exercises the negative against a local mutex.",
    ),
]

BY_ID = {m[0]: m for m in MUTATIONS}
FILES = sorted({m[1] for m in MUTATIONS})


def snapshot():
    """Take a fresh copy of the pristine files. Always overwrites.

    **This used to be `if not os.path.exists(dst)`, and that cost an hour.**
    A backup left behind by a previous sweep survived into the next one, so
    `--revert` restored a tree from before the intervening edits and silently
    threw two source fixes away — the same defect class as the mutation that
    does not apply, in the other direction. A scratch directory that is
    treated as an archive is an archive nobody chose.
    """
    shutil.rmtree(BACKUP, ignore_errors=True)
    os.makedirs(BACKUP)
    for f in FILES:
        shutil.copy2(os.path.join(ROOT, f), os.path.join(BACKUP, f.replace("/", "__")))


def revert():
    """Put the pristine files back, and **stamp them with the current time**.

    `shutil.copy2` preserves mtime, and that quietly broke a whole sweep run.
    Restoring a file gave it an mtime *older* than the object cargo had just
    built from the mutated source, so `rsync -a` carried the old stamp to the
    box and cargo decided the build was up to date — the control run after a
    sweep executed the **previous mutation's binary** and came back red for a
    tree that was pristine on disk.

    That is the sweep's own defect class arriving from inside the sweep: a
    measurement of something other than what was measured. Everything a
    mutation writes gets a fresh mtime for free because it is written rather
    than copied; only the restore needed saying out loud.
    """
    if not os.path.isdir(BACKUP):
        return 0
    for f in FILES:
        src = os.path.join(BACKUP, f.replace("/", "__"))
        if os.path.exists(src):
            dst = os.path.join(ROOT, f)
            shutil.copyfile(src, dst)
            os.utime(dst, None)
    return 0


def apply(mid):
    if not os.path.isdir(BACKUP):
        print(
            "no snapshot: run `plugin-hook-mutate.py --snapshot` first. Applying a "
            "mutation without one would leave no way back to the pristine tree.",
            file=sys.stderr,
        )
        return 3
    revert()
    _, path, old, new, _, _ = BY_ID[mid]
    full = os.path.join(ROOT, path)
    text = open(full).read()
    n = text.count(old)
    if n != 1:
        print(
            f"MUTATION {mid} does not apply: its anchor appears {n} times in "
            f"{path}, not once. The sweep is measuring nothing until this is "
            f"fixed — see ci/plugin-hook-falsify.sh's header on F3.",
            file=sys.stderr,
        )
        return 3
    open(full, "w").write(text.replace(old, new))
    return 0


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    if sys.argv[1] == "--list":
        print(json.dumps([{"id": m[0], "expect": m[4], "why": m[5]} for m in MUTATIONS]))
        return 0
    if sys.argv[1] == "--snapshot":
        snapshot()
        return 0
    if sys.argv[1] == "--revert":
        return revert()
    if sys.argv[1] == "--apply":
        return apply(sys.argv[2])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
