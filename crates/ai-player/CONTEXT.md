# ai-player

The Minesweeper binding for the agent runtime: how this game's Board is put in
front of the Agent. The Session is the Agent's, not this context's; what lives
here is policy — one Game drives one Agent, the InputMode bound by the first
committed Turn, a New Game ending the Session. That policy is in this context's
ADRs (0016, 0017).

## Language

**AiPlayer**:
A Player that plays a Game by deciding its moves with an Agent. One AiPlayer exists per app instance and holds one Agent; it begins and ends a Session through the Agent and never names one. Today it is driven by the HumanPlayer, who sends the Board and applies the reply.
_Avoid_: AI, agent, bot, AI opponent, guide, advisor

**InputMode**:
The way the Board is put in front of DeepSeek: `Plain` (the character grid), `Emoji`, or `Image` (a screenshot of the Board).
_Avoid_: BoardFormat, format, input format

**BoardView**:
The player-visible rendering of the Board that becomes the Agent's payload: each Cell's state and its neighbouring Mine count, in the symbols the active InputMode defines. It carries no Mine layout — a Hidden Cell's content is not readable through it.
_Avoid_: board payload, snapshot, grid, view
