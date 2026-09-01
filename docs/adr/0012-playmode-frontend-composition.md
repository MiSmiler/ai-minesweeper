# PlayMode: an exclusive per-mode composition that abandons the current Game on switch

Issues #91/#96 used `PlaySurface` as a throwaway working term; it was never a code symbol. We name the concept `PlayMode` — an open enum of the perspective a Game is shown from: `SinglePlay` (the player plays unaided), `AiGuide` (the player plays while a DeepSeek Guide suggests the next move), and later `AiPlay` / `AiPlayWithMe`.

A Game has exactly one PlayMode at a time. PlayModes are exclusive: switching PlayMode abandons the current Game and starts a fresh one (the backend still holds a single `Game`); there is no "resume the same board in the other mode." Each PlayMode is an independent composition mounted in `app/`, instantiating its own game slice (`createGameClient`) and, for the guide mode, an `ai/` slice; switching tears down and rebuilds the composition.

Considered: a single shared `#board` with toggleable guide panels — rejected because the guide game area wants its own copy of the game UI ("完全照搬"), and the later two-Board `AiPlayWithMe` mode needs instantiable units, not one re-parented board. The non-obvious kernel is that the modes *look* like two views of one board, but a switch loses the game.
