# ai-player

The Minesweeper binding for the agent runtime: how this game's Board is put in
front of the Agent, and the Session this product keeps per Game. What is policy
rather than vocabulary — one Session per Game, the InputMode bound by the first
committed Turn, a New Game ending the Session — lives in this context's ADRs
(0016, 0017).

## Language

**InputMode**:
The way the Board is put in front of DeepSeek: `Plain` (the character grid), `Emoji`, or `Image` (a screenshot of the Board). Distinct from **PlayMode**, which is the perspective the player sees.
_Avoid_: BoardFormat, format, input format

**BoardView**:
The player-visible rendering of the Board that becomes the Agent's payload: each Cell's state and its neighbouring Mine count, in the symbols the active InputMode defines. It carries no Mine layout — a Hidden Cell's content is not readable through it.
_Avoid_: board payload, snapshot, grid, view
