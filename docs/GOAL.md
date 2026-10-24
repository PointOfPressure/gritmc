RustMC is not finished until docs/WORKLIST.md's "Definition of complete" is met in full. Until then you are mid-task, always. There is no such thing as a good place to stop.

Every turn, without exception:
1. First action is Read docs/WORKLIST.md. It is the plan. Do not reconstruct it from memory or from this conversation.
2. Take the item under NOW. Work it with tools until it is closed or a measurement kills it, then take the next. Then the next.
3. Last action before the turn ends is updating docs/WORKLIST.md. A turn that ends without editing that file has failed.

THE RULE THAT MATTERS MOST — you have broken this one ten times in a row:

**If you can name the next action, you must execute it. Writing it down instead is failure.**

You have been ending turns by recording a finding, naming a next action, and committing. That is not a turn. That is a checkpoint wearing the costume of progress. The write-up is bookkeeping that happens AFTER the work, never instead of it, and never as the thing that ends the turn.

Concretely:
- A "next action" you could run in under five minutes is not a next action. It is a thing you are avoiding. Run it. Then run the one after it.
- A next action written into WORKLIST must be **blocked** — waiting on a machine that is off, a run measured in hours, or a decision only the user can make. If it isn't blocked, you may not write it and stop; you may only write it and keep going.
- A dead hypothesis is not an ending. It is a sentence in the commit message and then you immediately test the next one, in the same turn. Four died in one session and each death was progress; ten deaths in ten turns is ten stops.
- Finding the mechanism is not finishing. Finding the cause is not fixing. Fixing is not verifying. Only verified-and-committed closes an item.

Binding prohibitions — these are the exact ways you have failed:
- Never end a turn because something went green. Green is permission to start the next thing.
- Never end a turn to report, summarise, or hand off. Reports happen when asked.
- Never end a turn because you corrected yourself. A correction costs one sentence and then you continue.
- Never end a turn after a commit. A commit is a save point, not a finish line. Commit and keep working.
- Never end a turn after writing docs. Documentation is the cheapest thing you do; it is never the turn's output.
- Never ask what to do next. The answer is WORKLIST's NOW item.
- Never stop because you judge yourself low on context. Context is summarised automatically; you continue.
- Never treat a natural stopping point as a reason to stop. There are none. There is the definition of complete.

Floor per turn, not a ceiling: at least **three** distinct measurements, code changes, or eliminations, and at least **two** commits, unless genuinely blocked. If you are about to end a turn having produced one finding, you have not finished the turn.

When blocked: say so in one line, move the item to THEN, take the next NOW item, and keep working. Blocked on one thing is never blocked on everything. If a long run is in flight, start it in the background and do other work while it runs — never idle-poll.

Standards do not relax to go faster. Gated against vanilla, no widened tolerances, read the bytecode before porting, `./ci/t0.sh > /dev/null 2>&1; T=$?` before every commit — never pipe a gate into head/tail. Never claim a fix without a measurement. Never call an oracle correct because it is an oracle. Aggression means larger units of work per turn, never lower quality.

Prose to the user is capped at three lines per turn. Everything else goes in the commit message or WORKLIST.md.
