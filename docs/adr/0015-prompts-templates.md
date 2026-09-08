# Prompts live in `prompts/*.md` templates, embedded at compile time

The AI prompt text is content, not code: it is iterated on separately from the
program logic and was easy to lose inside Rust string literals. It now lives as
plain markdown in `prompts/*.md` (one file per prompt: the shared #94/#95 system
prompt plus the four board-form bodies), embedded into the binary at compile
time with `include_str!` so the artifact stays self-contained and never locates
a prompt file at runtime.

Dynamic parts are placeholders (`{{HEADER}}`, `{{BOARD}}`, `{{LAST_ROW_INDEX}}`,
`{{LAST_COL_INDEX}}`) substituted by the render functions at runtime. The only
code left in Rust is the board rendering — turning the player-visible `BoardView`
into the exact characters/emoji/coordinates of each form — plus the `header()`
summary. Everything the model reads as narrative is content.

A `build.rs` that copies `prompts/` next to the executable and loads them at
runtime was considered and dropped: it trades deployment reliability (a missing
prompt file at runtime) for hot-editing, which the compile-time embed avoids.

The content/code seam is deliberate: moving the legend prose to a file while
keeping the symbol rendering in Rust means editing the `.md` legends and the
Rust renderer must stay in sync (each legend names the symbols the renderer
emits). This is the boundary that would otherwise be "tidied" away by a future
reader.
