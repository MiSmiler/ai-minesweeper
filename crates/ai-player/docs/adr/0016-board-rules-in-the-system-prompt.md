# Board rules live in the system prompt; the user turn is the board alone

An `InputMode` picks both the board's rendering into the user turn and the
system-prompt section that describes it, so one system prompt carries every
rule — the coordinate system, the symbol legend, the output contract — and the
user message is the bare board grid (or, for `Image`, the screenshot alone).
The model is deliberately told neither the Mine count nor the Flag Budget: the
board is the whole input, and a tool that answers such questions on demand is
future work. The reply contract is reasoning plus a trailing `(row,col)`
coordinate, replacing the earlier `SUGGEST {"row":N,"col":M}` line.

Considered alternatives: a per-request header carrying the difficulty, Mine
count, and Flags Remaining was dropped — it would make the user turn more than
the board, duplicating what the player-visible grid already shows; injecting a
static `dims → mine count` table into the system prompt was dropped as well,
since the Mine count is what the future query tool is for.

Consequences: the system prompt is static per mode (byte-locked by tests, and
cacheable); the model cannot do endgame mine-counting arguments until the tool
lands; the frontend renders the `Image` mode's player bubble from its own
captured screenshot, because the backend echo for that turn is empty.
