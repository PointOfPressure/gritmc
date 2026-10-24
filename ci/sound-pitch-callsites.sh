#!/usr/bin/env bash
# Print the bytecode of one method from the pinned vanilla jar.
#
# Clean-room helper for the `sound.pitch` classification: every entry in
# `tools/oracle-runner/src/pitch.rs` is justified by a call site read through
# this script, never from memory of an older version and never from any other
# implementation's source. The table lives in Rust rather than in a
# `ci/checks/*.toml` because its consumer is the recorder, not a python gate,
# and a toml would only be the same constants written twice.
#
#   ci/sound-pitch-callsites.sh <fully.qualified.Class> <methodName>
#
# With no method it lists the method headers so you can pick one, which is the
# cheap way to avoid dumping a 157 KB class to answer a question about nine
# instructions (`docs/AGENT-BRIEF.md`, output hygiene).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JAR="$ROOT/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
[ -f "$JAR" ] || { echo "no pinned jar at $JAR" >&2; exit 2; }

CLASS="${1:?usage: $0 <class> [method]}"
METHOD="${2:-}"

if [ -z "$METHOD" ]; then
    javap -p -classpath "$JAR" "$CLASS" | sed -n 's/^  \(.*\);$/\1/p'
    exit 0
fi

# `javap -c` emits one method header per method followed by its Code block.
# Print from the header naming $METHOD up to the next header at the same
# indent. `-constants` is what makes the float literals readable as values
# rather than as constant-pool indices, which is the entire point here.
javap -p -c -constants -classpath "$JAR" "$CLASS" |
    awk -v m="$METHOD" '
        /^  [^ ].*\(/ { inm = (index($0, " " m "(") > 0) }
        inm { print }
    '
