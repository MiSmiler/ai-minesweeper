//! The tools the harness exposes to DeepSeek. So far just the `read_board`
//! tool: it returns the current Board as compact text the model reads to
//! reason about the position.

use serde_json::json;

use crate::core::{CellContent, CellState, CellView, Game, Position};
use crate::server::{difficulty_str, game_state_str};

/// The single-character glyph for a Cell: `.` hidden, `F` flagged, `0`-`8`
/// revealed neighbor count (a zero shows `0`, not blank, so it is never
/// mistaken for hidden), `*` a revealed Mine.
fn glyph(view: CellView) -> char {
    match view.state {
        CellState::Hidden => '.',
        CellState::Flagged => 'F',
        CellState::Revealed => match view.content {
            Some(CellContent::Number(n)) => (b'0' + n.min(8)) as char,
            Some(CellContent::Mine) => '*',
            // Revealed Cells always carry content (core invariant); defensive.
            None => '?',
        },
    }
}

/// The number of ASCII digits needed to render `n` (`0` counts as one).
fn digit_width(n: usize) -> usize {
    n.max(1).to_string().len()
}

/// Renders the current `Game` as the compact Board text the model reads:
/// a metadata line (`board`, `game_state`, `difficulty`, `flags_remaining`),
/// a blank line, a column header, then one line per row (row label plus the
/// Cell glyphs). Rows and columns are 0-indexed, matching `Position`.
#[allow(dead_code)] // wired by the `/ai/analyze` session (T4)
pub fn board_snapshot_text(game: &Game) -> String {
    let size = game.size();
    let rows = size.rows;
    let cols = size.cols;
    let row_w = digit_width(rows.saturating_sub(1));
    let col_w = digit_width(cols.saturating_sub(1));

    let mut out = String::new();
    out.push_str(&format!(
        "board {rows}x{cols}  game_state={}  difficulty={}  flags_remaining={}\n\n",
        game_state_str(game.game_state()),
        difficulty_str(game.difficulty()),
        game.flags_remaining(),
    ));

    // Column header: right-aligned indices over the Cell columns.
    for _ in 0..(row_w + 1) {
        out.push(' ');
    }
    for col in 0..cols {
        out.push_str(&format!("{col:>col_w$} "));
    }
    out.push('\n');

    for row in 0..rows {
        out.push_str(&format!("{row:>row_w$} "));
        for col in 0..cols {
            let view = game.cell_view(Position::new(row, col));
            out.push_str(&format!("{} ", glyph(view)));
        }
        out.pop(); // drop the trailing space
        out.push('\n');
    }

    out
}

/// The `read_board` tool definition in the OpenAI/DeepSeek `tools` format:
/// no arguments, returns the Board text from `board_snapshot_text`.
#[allow(dead_code)] // wired by the `/ai/analyze` session (T4)
pub fn read_board_tool() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "read_board",
            "description": "Reads the current Minesweeper board as compact text. Rows and columns are 0-indexed. Glyphs: `.` hidden, a digit is the revealed neighbor count (0 means empty and revealed), `F` flagged, `*` a revealed mine.",
            "parameters": { "type": "object", "properties": {} }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Difficulty, GameMode};

    fn view(state: CellState, content: Option<CellContent>) -> CellView {
        CellView { state, content }
    }

    #[test]
    fn glyph_hidden_is_dot() {
        assert_eq!(glyph(view(CellState::Hidden, None)), '.');
    }

    #[test]
    fn glyph_flagged_is_f() {
        assert_eq!(glyph(view(CellState::Flagged, None)), 'F');
    }

    #[test]
    fn glyph_revealed_number_is_its_digit() {
        assert_eq!(
            glyph(view(CellState::Revealed, Some(CellContent::Number(0)))),
            '0'
        );
        assert_eq!(
            glyph(view(CellState::Revealed, Some(CellContent::Number(8)))),
            '8'
        );
    }

    #[test]
    fn glyph_revealed_mine_is_star() {
        assert_eq!(
            glyph(view(CellState::Revealed, Some(CellContent::Mine))),
            '*'
        );
    }

    #[test]
    fn board_text_reports_metadata_and_hidden_grid() {
        let game = Game::new(Difficulty::Beginner, GameMode::Classic);
        let text = board_snapshot_text(&game);
        assert!(text.contains("board 9x9"));
        assert!(text.contains("game_state=ready"));
        assert!(text.contains("difficulty=beginner"));
        assert!(text.contains("flags_remaining=10"));
        assert!(
            text.lines()
                .nth(2)
                .unwrap()
                .starts_with("  0 1 2 3 4 5 6 7 8")
        );
        assert_eq!(text.lines().nth(3), Some("0 . . . . . . . . ."));
    }

    #[test]
    fn board_text_shows_a_revealed_number() {
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            GameMode::Classic,
            &[Position::new(0, 1), Position::new(1, 0)],
        );
        game.reveal(Position::new(0, 0));
        let text = board_snapshot_text(&game);
        assert!(text.contains("game_state=playing"));
        // (0,0) has exactly two Mine neighbours (0,1) and (1,0) -> reveals 2;
        // the rest of row 0 stays hidden.
        assert_eq!(text.lines().nth(3), Some("0 2 . . . . . . . ."));
    }

    #[test]
    fn read_board_tool_declares_no_arguments() {
        let tool = read_board_tool();
        assert_eq!(tool["function"]["name"], "read_board");
        assert_eq!(
            tool["function"]["parameters"]["type"],
            serde_json::Value::String("object".into())
        );
    }
}
