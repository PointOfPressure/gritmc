#!/usr/bin/env bash
# chat-commands-falsify.sh — inject known bugs; every one must be caught.
#
# A test suite that passes proves nothing about a test suite that *can* fail.
# This mutates the chat and command-registry sources one bug at a time, runs
# the crate's unit tests, and reports any injection the tests did not catch.
#
# **Includes a control.** The last two entries change nothing observable — a
# comment and a local variable name — and the sweep FAILS if either is
# reported as caught. Without that, a suite that had simply stopped compiling,
# or a runner that reported failure unconditionally, would score 100% and look
# like the strongest result in the file. Two of this project's sweeps caught
# their own instrument before they caught any real defect.
#
# Usage (on the box; nothing here compiles on the laptop):
#     ./ci/chat-commands-falsify.sh
#
# Exit 0 only if every real injection was caught and both controls survived.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

# **The script puts cargo on its own PATH rather than trusting its launcher.**
# Run under `setsid nohup` from a non-interactive ssh, `$HOME/.cargo/bin` is
# absent, every `cargo test` exits 127, every case scores as *caught*, and the
# sweep reports a perfect score for a run in which nothing was compiled. That
# happened on 2026-08-06 and only the two controls caught it.
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null || { echo "no cargo on PATH" >&2; exit 2; }

# **The baseline must be green before a single bug is injected.** Without
# this, any pre-existing failure — a missing table, a broken build, a cargo
# that is not there — makes every injection look caught. The controls at the
# bottom detect it too, but only after forty minutes of compiling; this
# detects it in one.
echo "baseline: the unmutated tree must pass"
if ! cargo test --release -p rustmc-server --lib >/tmp/falsify-baseline.txt 2>&1; then
    echo "BASELINE IS RED — every injection would score as caught. Aborting." >&2
    grep -E "^error|panicked|test result|not found" /tmp/falsify-baseline.txt | head -10 >&2
    exit 2
fi
grep -E "test result" /tmp/falsify-baseline.txt | head -2

CHAT=crates/rustmc-server/src/chat.rs
TREE=crates/rustmc-server/src/commandtree.rs
CMDS=crates/rustmc-server/src/commands.rs
PLAY=crates/rustmc-server/src/players.rs
FILES=("$CHAT" "$TREE" "$CMDS" "$PLAY")

BACKUP="$(mktemp -d)"
trap 'for f in "${FILES[@]}"; do cp "$BACKUP/$(basename "$f")" "$ROOT/$f"; done; rm -rf "$BACKUP"' EXIT
for f in "${FILES[@]}"; do cp "$ROOT/$f" "$BACKUP/$(basename "$f")"; done

restore() { for f in "${FILES[@]}"; do cp "$BACKUP/$(basename "$f")" "$ROOT/$f"; done; }

# apply <file> <python-repr-old> <python-repr-new>
apply() {
    python3 - "$1" "$2" "$3" <<'PY'
import sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(path).read()
if s.count(old) != 1:
    sys.exit(f"ANCHOR MISS in {path}: {s.count(old)} occurrences")
open(path, 'w').write(s.replace(old, new))
PY
}

PASSED=0
MISSED=0
declare -a MISSES

# name | file | old | new | expectation(catch|control)
run_case() {
    local name="$1" file="$2" old="$3" new="$4" expect="$5"
    restore
    if ! apply "$file" "$old" "$new" 2>/tmp/falsify-anchor.txt; then
        echo "  !! $name — $(cat /tmp/falsify-anchor.txt)"
        MISSED=$((MISSED+1)); MISSES+=("$name (anchor)"); return
    fi
    if cargo test --release -p rustmc-server --lib >/tmp/falsify-out.txt 2>&1; then
        result=survived
    else
        result=caught
    fi
    if [ "$expect" = control ]; then
        if [ "$result" = survived ]; then
            echo "  ok  CONTROL $name — survived, as it must"
            PASSED=$((PASSED+1))
        else
            echo "  !!  CONTROL $name — reported as caught; THE SWEEP IS LYING"
            MISSED=$((MISSED+1)); MISSES+=("CONTROL $name")
        fi
    elif [ "$result" = caught ]; then
        echo "  ok  $name"
        PASSED=$((PASSED+1))
    else
        echo "  !!  $name — NOT CAUGHT"
        MISSED=$((MISSED+1)); MISSES+=("$name")
    fi
}

echo "falsification sweep: chat + command registry"

# ---- the command tree: numbering, flags, ordering, filtering -------------
run_case "numbering: breadth-first becomes depth-first" "$TREE" \
    'if !k.children.is_empty() {
                    queue.push_back((idx, &k.children));
                }' \
    'if !k.children.is_empty() {
                    queue.push_front((idx, &k.children));
                }' catch

run_case "children: registration order reversed" "$TREE" \
    '            for k in kids {' \
    '            for k in kids.iter().rev() {' catch

run_case "flags: RESTRICTED never set" "$TREE" \
    '                    if n.restricted() {
                        f |= FLAG_RESTRICTED;
                    }' \
    '                    if false && n.restricted() {
                        f |= FLAG_RESTRICTED;
                    }' catch

run_case "flags: EXECUTABLE never set" "$TREE" \
    '                    if n.executable {
                        f |= FLAG_EXECUTABLE;
                    }' \
    '                    if false && n.executable {
                        f |= FLAG_EXECUTABLE;
                    }' catch

run_case "flags: a literal is typed as a root" "$TREE" \
    '                    let mut f = TYPE_LITERAL;' \
    '                    let mut f = TYPE_ROOT;' catch

run_case "rootIndex is 1 instead of 0" "$TREE" \
    '        write_varint(&mut out, 0)?;
        Ok(out)' \
    '        write_varint(&mut out, 1)?;
        Ok(out)' catch

run_case "permission filter removed entirely" "$TREE" \
    '            .filter(|n| level.allows(n.level))' \
    '            .filter(|n| level.allows(n.level) || true)' catch

run_case "filter stops recursing (flat instead of pruning subtrees)" "$TREE" \
    '                children: Self::usable(&n.children, level),' \
    '                children: n.children.clone(),' catch

run_case "restricted is relative to the recipient, not absolute" "$TREE" \
    '        self.level != Level::All' \
    '        false' catch

run_case "permission byte is 24+level+1" "$TREE" \
    '        24 + self as u8' \
    '        25 + self as u8' catch

run_case "allows() becomes strictly greater" "$TREE" \
    '        self >= required' \
    '        self > required' catch

# ---- the registry and its replies ---------------------------------------
run_case "sibling literals swap places in the root" "$CMDS" \
    '            Node::leaf("list").then(Node::leaf("uuids")),' \
    '            Node::leaf("uuids").then(Node::leaf("list")),' catch

# Note the shape of this one: the replacement must COMPILE. An injection that
# only fails to build is caught by rustc, not by a test, and proves nothing
# about the suite. `let bracketed = inner;` is a legal rebinding that drops
# the `chat.square_brackets` translation wrapper and nothing else.
run_case "/seed loses its square-bracket wrapper" "$CMDS" \
    '    let bracketed = compound([
        ("with", Nbt::List(vec![inner])),
        ("translate", Nbt::String("chat.square_brackets".into())),
    ]);' \
    '    let bracketed = inner;' catch

run_case "/list uuids arguments become {text: ...} compounds" "$CMDS" \
    '                Nbt::String(name.to_string()),
                Nbt::String(dashed_uuid(name)),' \
    '                compound([("text", Nbt::String(name.to_string()))]),
                compound([("text", Nbt::String(dashed_uuid(name)))]),' catch

run_case "uuid loses its dashes" "$CMDS" \
    '        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",' \
    '        "{:08x}{:04x}{:04x}{:04x}{:012x}",' catch

run_case "/seed reply ignores its argument" "$CMDS" \
    '    let text = seed.to_string();' \
    '    let text = 0i64.to_string();' catch

run_case "list max-players argument dropped" "$CMDS" \
    '                primitive_arg(MAX_PLAYERS),' \
    '                primitive_arg(online),' catch

# ---- chat ----------------------------------------------------------------
run_case "last-seen bitset read as 2 bytes not 3" "$CHAT" \
    'const LAST_SEEN_BITS: usize = 20;' \
    'const LAST_SEEN_BITS: usize = 16;' catch

run_case "chat type holder written as the bare id" "$CHAT" \
    '    write_varint(&mut o, CHAT_TYPE_ID + 1)?;' \
    '    write_varint(&mut o, CHAT_TYPE_ID)?;' catch

run_case "salt echoed instead of zeroed" "$CHAT" \
    '    wire::write_i64(&mut o, 0)?;
    // `LastSeenMessages.EMPTY.pack(...)` is an empty collection.' \
    '    wire::write_i64(&mut o, timestamp_millis)?;
    // `LastSeenMessages.EMPTY.pack(...)` is an empty collection.' catch

run_case "signature written as present" "$CHAT" \
    '    // `writeNullable(signature, ...)` — absent.
    wire::write_bool(&mut o, false)?;' \
    '    // `writeNullable(signature, ...)` — absent.
    wire::write_bool(&mut o, true)?;' catch

run_case "message index confused with globalIndex" "$CHAT" \
    '    write_varint(&mut o, 0)?;
    // `writeNullable(signature, ...)` — absent.' \
    '    write_varint(&mut o, global_index)?;
    // `writeNullable(signature, ...)` — absent.' catch

run_case "SYSTEM visibility also receives chat" "$CHAT" \
    '        self == Self::Full' \
    '        self != Self::Hidden' catch

run_case "the section sign is allowed in chat" "$CHAT" \
    "    c != '\\u{a7}' && c >= ' ' && c != '\\u{7f}'" \
    "    c >= ' ' && c != '\\u{7f}'" catch

run_case "control characters allowed in chat" "$CHAT" \
    "    c != '\\u{a7}' && c >= ' ' && c != '\\u{7f}'" \
    "    c != '\\u{a7}'" catch

run_case "globalIndex pre-increments instead of post" "$PLAY" \
    '        self.next_chat_index.fetch_add(1, Ordering::AcqRel)' \
    '        self.next_chat_index.fetch_add(1, Ordering::AcqRel) + 1' catch

# ---- CONTROLS: must survive ---------------------------------------------
run_case "a comment is reworded" "$CHAT" \
    '/// `readUtf(256)` — the message length cap `ServerboundChatPacket` reads with.' \
    '/// The message length cap. (Control: this comment change must not fail.)' control

run_case "a local variable is renamed" "$TREE" \
    '        let filtered = Self::usable(&self.roots, level);
        let flat = Self::enumerate(&filtered);' \
    '        let kept = Self::usable(&self.roots, level);
        let flat = Self::enumerate(&kept);' control

restore
echo
TOTAL=$((PASSED+MISSED))
echo "falsification: $PASSED/$TOTAL"
if [ "$MISSED" -ne 0 ]; then
    echo "NOT CAUGHT / instrument failures:"
    for m in "${MISSES[@]}"; do echo "  - $m"; done
    exit 1
fi
