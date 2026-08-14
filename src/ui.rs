//! UI layer: ratatui rendering + crossterm event mapping.
//!
//! A thin adapter over the core interface. Layout and hit-testing are
//! unit-tested; rendering and terminal I/O are verified manually.

use ratatui::crossterm::event::MouseEvent;
use ratatui::layout::{Position as TermPos, Rect};
use ratatui::prelude::{Color, Frame, Modifier, Style};
use ratatui::widgets::Paragraph;

use crate::core::{
    BoardSize, CellContent, CellState, Difficulty, Game, GameMode, GameState, Position,
};

/// Clickable regions of the top bar and the board area.
#[derive(Clone, Copy)]
pub struct UiLayout {
    pub difficulty_buttons: [(Difficulty, Rect); 3],
    pub flag_counter: Rect,
    pub timer: Rect,
    pub new_game_button: Rect,
    pub board: Rect,
}

const NEW_GAME: &str = " New Game ";

fn label(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Beginner => " Beginner ",
        Difficulty::Intermediate => " Intermediate ",
        Difficulty::Expert => " Expert ",
    }
}

impl UiLayout {
    /// Computes the layout of the whole screen. Pure function of the board
    /// size, so both rendering and event hit-testing use it.
    pub fn for_size(size: BoardSize) -> UiLayout {
        let board = Rect::new(0, 1, (size.cols * 2) as u16, size.rows as u16);

        // Top bar elements, left to right: difficulty buttons, flag counter, timer, New Game.
        let mut x: u16 = 0;
        let mut difficulty_buttons = [(Difficulty::Beginner, Rect::ZERO); 3];
        for (i, difficulty) in [
            Difficulty::Beginner,
            Difficulty::Intermediate,
            Difficulty::Expert,
        ]
        .into_iter()
        .enumerate()
        {
            let width = label(difficulty).len() as u16;
            difficulty_buttons[i] = (difficulty, Rect::new(x, 0, width, 1));
            x += width;
        }
        let flag_counter = Rect::new(x, 0, 9, 1);
        x += flag_counter.width;
        let timer = Rect::new(x, 0, 6, 1);
        x += timer.width;
        let new_game_button = Rect::new(x, 0, NEW_GAME.len() as u16, 1);

        UiLayout {
            difficulty_buttons,
            flag_counter,
            timer,
            new_game_button,
            board,
        }
    }
}

/// Whether a board of the given size fits in the terminal area.
pub fn fits_in(size: BoardSize, area: Rect) -> bool {
    area.width >= (size.cols * 2) as u16 && area.height > size.rows as u16
}

/// Renders the whole screen.
pub fn render(frame: &mut Frame, game: &Game) {
    let area = frame.area();
    let layout = UiLayout::for_size(game.size());

    if !fits_in(game.size(), area) {
        frame.render_widget(
            Paragraph::new("Terminal too small to fit the board")
                .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    render_top_bar(frame, &layout, game);
    render_board(frame, layout.board, game);
}

fn render_top_bar(frame: &mut Frame, layout: &UiLayout, game: &Game) {
    for (difficulty, rect) in &layout.difficulty_buttons {
        let selected = *difficulty == game.difficulty();
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        frame.render_widget(Paragraph::new(label(*difficulty)).style(style), *rect);
    }

    frame.render_widget(
        Paragraph::new(format!("Flags:{:>3}", game.flags_remaining()))
            .style(Style::default().fg(Color::Yellow)),
        layout.flag_counter,
    );

    let secs = game.elapsed().as_secs();
    let timer = format!("{:02}:{:02}", secs / 60, secs % 60);
    frame.render_widget(
        Paragraph::new(timer).style(Style::default().fg(Color::Cyan)),
        layout.timer,
    );

    frame.render_widget(
        Paragraph::new(NEW_GAME).style(Style::default().fg(Color::Black).bg(Color::Blue)),
        layout.new_game_button,
    );

    // Result banner on the right side of the top bar, if the game has ended.
    let (text, color) = match game.game_state() {
        GameState::Won => ("WON", Color::Green),
        GameState::Lost => ("LOST", Color::Red),
        _ => return,
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Rect::new(layout.new_game_button.right() + 2, 0, text.len() as u16, 1),
    );
}

fn render_board(frame: &mut Frame, board: Rect, game: &Game) {
    let buf = frame.buffer_mut();
    let size = game.size();
    for row in 0..size.rows {
        for col in 0..size.cols {
            let x = board.x + (col as u16) * 2;
            let y = board.y + row as u16;
            let view = game.cell_view(Position::new(row, col));
            let (symbol, style) = cell_style(
                view.state,
                view.content,
                game.is_trigger(Position::new(row, col)),
            );
            // Multi-width symbols (🚩, 💣) fill both cells; wide 2 spaces cover the rest.
            buf.set_stringn(x, y, symbol, 2, style);
            if let Some(CellContent::Number(n)) = view.content {
                buf.set_stringn(
                    x + 1,
                    y,
                    n.to_string(),
                    1,
                    Style::default().fg(number_color(n)).bg(Color::White),
                );
            }
        }
    }
}

fn cell_style(
    state: CellState,
    content: Option<CellContent>,
    is_trigger: bool,
) -> (&'static str, Style) {
    match (state, content) {
        (CellState::Hidden, _) => ("  ", Style::default().bg(Color::DarkGray)),
        (CellState::Flagged, _) => ("🚩", Style::default().bg(Color::DarkGray)),
        (CellState::Revealed, Some(CellContent::Mine)) => {
            let style = if is_trigger {
                Style::default().bg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(Color::Rgb(139, 0, 0))
            };
            ("💣", style)
        }
        (CellState::Revealed, Some(CellContent::Number(_))) => {
            ("  ", Style::default().bg(Color::White))
        }
        (CellState::Revealed, None) => ("  ", Style::default().bg(Color::White)),
    }
}

fn number_color(n: u8) -> Color {
    match n {
        1 => Color::Blue,
        2 => Color::Green,
        3 => Color::Red,
        4 => Color::Rgb(0, 0, 160),
        5 => Color::Rgb(160, 0, 0),
        6 => Color::Cyan,
        7 => Color::Black,
        8 => Color::DarkGray,
        _ => Color::White,
    }
}

/// Maps a mouse event to a core action. All clicks are ignored while the
/// terminal is too small to fit the board, so the input path matches what
/// is rendered.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent, area: Rect) {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    if !fits_in(app.game.size(), area) {
        return;
    }

    let mouse_pos = TermPos::new(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => match hit_test(app.layout, mouse_pos) {
            Some(Hit::NewGame) => app.reset(),
            Some(Hit::Difficulty(d)) => {
                app.game = Game::new(d, app.mode);
                app.right_pressed = false;
            }
            Some(Hit::Cell(pos)) => {
                if app.right_pressed && app.game.game_state() == GameState::Playing {
                    app.game.chord(pos);
                } else {
                    app.game.reveal(pos);
                }
            }
            None => {}
        },
        MouseEventKind::Down(MouseButton::Right) => {
            app.right_pressed = true;
            if let Some(Hit::Cell(pos)) = hit_test(app.layout, mouse_pos) {
                app.game.toggle_flag(pos);
            }
        }
        MouseEventKind::Up(MouseButton::Right) => {
            app.right_pressed = false;
        }
        _ => {}
    }
}

/// Whether the key asks the app to quit: only app-lifecycle keys (q / Ctrl+C)
/// — the game itself is mouse-only by design.
pub fn should_quit(key: ratatui::crossterm::event::KeyEvent) -> bool {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Char('q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}

/// What a click landed on: a top-bar control or a board Cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    NewGame,
    Difficulty(Difficulty),
    Cell(Position),
}

/// Maps a terminal position to the interactive element under it: a top-bar
/// control or a board Cell. Returns `None` when the position is on empty
/// space. Board Cells are rendered two terminal columns wide, so the column
/// is derived by halving the horizontal offset from the board origin.
fn hit_test(layout: UiLayout, pos: TermPos) -> Option<Hit> {
    if layout.new_game_button.contains(pos) {
        return Some(Hit::NewGame);
    }
    for (difficulty, rect) in layout.difficulty_buttons {
        if rect.contains(pos) {
            return Some(Hit::Difficulty(difficulty));
        }
    }
    if pos.x < layout.board.x || pos.y < layout.board.y {
        return None;
    }
    let col = ((pos.x - layout.board.x) / 2) as usize;
    let row = (pos.y - layout.board.y) as usize;
    if col >= (layout.board.width / 2) as usize || row >= layout.board.height as usize {
        return None;
    }
    Some(Hit::Cell(Position::new(row, col)))
}

/// Application state: the game plus UI-only state.
pub struct App {
    pub game: Game,
    /// The mode every new game is created with, set once at launch.
    pub mode: GameMode,
    pub right_pressed: bool,
    layout: UiLayout,
}

impl App {
    pub fn new(mode: GameMode) -> Self {
        Self {
            game: Game::new(Difficulty::Beginner, mode),
            mode,
            right_pressed: false,
            layout: UiLayout {
                difficulty_buttons: [(Difficulty::Beginner, Rect::ZERO); 3],
                flag_counter: Rect::ZERO,
                timer: Rect::ZERO,
                new_game_button: Rect::ZERO,
                board: Rect::ZERO,
            },
        }
    }

    pub fn update_layout(&mut self) {
        self.layout = UiLayout::for_size(self.game.size());
    }

    fn reset(&mut self) {
        self.game = Game::new(self.game.difficulty(), self.mode);
        self.right_pressed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_size_matches_difficulty_boards() {
        assert_eq!(
            UiLayout::for_size(Difficulty::Beginner.size()).board,
            Rect::new(0, 1, 18, 9)
        );
        assert_eq!(
            UiLayout::for_size(Difficulty::Intermediate.size()).board,
            Rect::new(0, 1, 32, 16)
        );
        assert_eq!(
            UiLayout::for_size(Difficulty::Expert.size()).board,
            Rect::new(0, 1, 60, 16)
        );
    }

    #[test]
    fn fits_in_at_boundaries() {
        // Expert: 30 cols x 16 rows -> needs 60 wide and height > 16.
        let size = Difficulty::Expert.size();
        assert!(fits_in(size, Rect::new(0, 0, 60, 17)));
        assert!(!fits_in(size, Rect::new(0, 0, 59, 17)));
        assert!(!fits_in(size, Rect::new(0, 0, 60, 16)));
    }

    #[test]
    fn hit_test_top_bar_controls() {
        let layout = UiLayout::for_size(Difficulty::Beginner.size());
        assert_eq!(
            hit_test(layout, TermPos::new(2, 0)),
            Some(Hit::Difficulty(Difficulty::Beginner))
        );
        assert_eq!(
            hit_test(layout, TermPos::new(12, 0)),
            Some(Hit::Difficulty(Difficulty::Intermediate))
        );
        assert_eq!(
            hit_test(layout, TermPos::new(26, 0)),
            Some(Hit::Difficulty(Difficulty::Expert))
        );
        assert_eq!(
            hit_test(layout, TermPos::new(layout.new_game_button.x + 1, 0)),
            Some(Hit::NewGame)
        );
    }

    #[test]
    fn hit_test_board_cells() {
        let layout = UiLayout::for_size(Difficulty::Beginner.size());
        assert_eq!(
            hit_test(layout, TermPos::new(0, 1)),
            Some(Hit::Cell(Position::new(0, 0)))
        );
        // Each Cell spans two terminal columns.
        assert_eq!(
            hit_test(layout, TermPos::new(2, 1)),
            Some(Hit::Cell(Position::new(0, 1)))
        );
        assert_eq!(
            hit_test(layout, TermPos::new(1, 2)),
            Some(Hit::Cell(Position::new(1, 0)))
        );
    }

    #[test]
    fn hit_test_empty_space_is_none() {
        let layout = UiLayout::for_size(Difficulty::Beginner.size());
        // The flag counter / timer strip is not clickable.
        assert_eq!(hit_test(layout, TermPos::new(33, 0)), None);
        // Right of the board.
        assert_eq!(hit_test(layout, TermPos::new(18, 1)), None);
        // Below the board.
        assert_eq!(hit_test(layout, TermPos::new(0, 10)), None);
    }
}
