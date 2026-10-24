#!/usr/bin/env python3
"""Turn a vanilla translation template into a grep -E pattern, from the jar.

    unzip -p <jar> assets/minecraft/lang/en_us.json \
        | ci/lang-message-re.py commands.time.set.absolute 6000

Prints one extended-regex line matching what a vanilla server writes to its
log for that message, with the LAST `%s` pinned to the value given and every
earlier one left as `\\S+`. Exits 2 if the key is absent, 3 if the template
does not take exactly the number of arguments asked for.

# WHY THIS EXISTS

`ci/t2-gameplay.sh` confirmed `time set 6000` by grepping for
`Set the time to 6000`. 26.2 prints `Set minecraft:overworld to 6000
tick(s)` — `commands.time.set.absolute`, which is what
`net/minecraft/server/commands/TimeCommand.class` references twice, while
`commands.time.set` is referenced nowhere and yet is STILL PRESENT in the
lang file with the old wording. So the string was recalled from an older
version, the harness refused a vanilla server that had done exactly what it
was asked, and it cost a 36-minute corpus-lock wait and a whole T2 slot on
2026-09-05.

The general rule this mechanises is the one `docs/AGENT-BRIEF.md` states for
the snake_case game rules, which were discovered twice and repeated a third
time here: **a console command whose confirmation is written down rather
than derived is a confirmation for whichever version the author remembers.**
Deriving it from the pinned jar makes a wording change move the predicate
instead of silently emptying it, and makes a vanished key a refusal.

# WHAT IT CANNOT DO

It does not know which key a command uses — that comes from the bytecode and
is the caller's to name. Two keys can carry the same text, and in this case
one of them is dead. A lang grep for the *text* would have found the wrong
one, which is why the caller passes a key and not a phrase.
"""
import json
import re
import sys


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: lang-message-re.py <key> [value ...]", file=sys.stderr)
        return 2
    key, values = sys.argv[1], sys.argv[2:]
    lang = json.load(sys.stdin)
    tmpl = lang.get(key)
    if tmpl is None:
        print(f"lang-message-re: no {key} in this jar's en_us.json", file=sys.stderr)
        return 2
    parts = tmpl.split("%s")
    slots = len(parts) - 1
    if slots < len(values):
        print(f"lang-message-re: {key} = {tmpl!r} takes {slots} argument(s), "
              f"{len(values)} value(s) given", file=sys.stderr)
        return 3
    # Values fill the LAST slots; anything before them is a wildcard. For
    # 'Set %s to %s tick(s)' with one value that pins the tick count and
    # leaves the clock free, which is the right strength: the command names
    # no clock, so which one it resolves to is the substrate's claim and not
    # this predicate's.
    fills = [r"\S+"] * (slots - len(values)) + [re.escape(v) for v in values]
    out = re.escape(parts[0])
    for fill, tail in zip(fills, parts[1:]):
        out += fill + re.escape(tail)
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
