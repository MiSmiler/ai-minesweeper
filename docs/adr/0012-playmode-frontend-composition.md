# PlayMode: an exclusive per-mode composition that abandons the current Game on switch

> Superseded by ADR-0023: the PlayMode split is gone — there is one page, the AI panel sits beside the Board, and no switch abandons a Game. Read the decisions below as history; `HumanPlayer` / `AiPlayer` survive as glossary concepts.

> Reopened by ADR-0019: the two members are now `HumanPlay` (was `SinglePlay`) and `AiPlay` (was `AiHelpMePlay`, then `AiGuide` in the code), and ADR-0019 states their meanings — the body below is amended to match. The decisions about exclusivity and composition are unchanged.

Issues #91/#96 used `PlaySurface` as a throwaway working term; it was never a code symbol. We name the concept `PlayMode` — an open enum of the perspective a Game is shown from: `HumanPlay` (the HumanPlayer plays unaided), `AiPlay` (an AiPlayer plays the Game, driven by hand until the tool loop lands — ADR-0019), and later `HumanVsAiPlay`.

A Game has exactly one PlayMode at a time. PlayModes are exclusive: switching PlayMode abandons the current Game and starts a fresh one (the backend still holds a single `Game`); there is no "resume the same board in the other mode." Each PlayMode is an independent composition mounted in `app/`, instantiating its own game slice (`createGameClient`) and, for assist, an `ai/` slice; switching tears down and rebuilds the composition.

Considered: a single shared `#board` with toggleable assist panels — rejected because the assist game area wants its own copy of the game UI ("完全照搬"), and the later two-Board `HumanVsAiPlay` mode needs instantiable units, not one re-parented board. The non-obvious kernel is that the modes *look* like two views of one board, but a switch loses the game.
