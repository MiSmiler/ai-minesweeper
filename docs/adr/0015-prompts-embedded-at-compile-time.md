# Prompts live in `prompts/*.md`, embedded at compile time

The AI prompt text is content, not code: it is iterated on separately from the
program logic and was easy to lose inside Rust string literals. It now lives as
plain markdown in `prompts/*.md` — the shared system-prompt core
(`prompts/system.md`) plus one section per `InputMode` (ADR-0016: `plain.md`,
`emoji.md`, `image.md`) — embedded into the binary at compile time with
`include_str!` so the artifact stays self-contained and never locates a prompt
file at runtime.

Every file is static text: an `InputMode`'s system prompt is the core plus its
section, and the user turn is rendered entirely in Rust (the `BoardView` into
the mode's symbols). There are no placeholders left; prompt prose and board
rendering meet only in `Guide::suggest`.

A `build.rs` that copies `prompts/` next to the executable and loads them at
runtime was considered and dropped: it trades deployment reliability (a missing
prompt file at runtime) for hot-editing, which the compile-time embed avoids.

The content/code seam is deliberate: the legend prose lives in a file while the
symbol rendering stays in Rust, so editing a `.md` legend and the Rust renderer
must stay in sync (each legend names the symbols the renderer emits). This is
the boundary that would otherwise be "tidied" away by a future reader.
