//! UI layer: ratatui rendering + crossterm event mapping.
//!
//! A thin adapter over the core interface. Not unit-tested — terminal I/O is
//! verified manually.

use ratatui::crossterm::event::MouseEvent;
use ratatui::layout::{Position as TermPos, Rect};
use ratatui::prelude::{Color, Frame, Modifier, Style};
use ratatui::widgets::Paragraph;

use crate::core::{CellContent, CellState, Difficulty, Game, GameState, Position};

/// Clickable regions of the top bar and the board area.
#[derive(Clone, Copy)]
pub struct UiLayout {
    pub difficulty_buttons: [(Difficulty, Rect); 3],
    pub mine_counter: Rect,
    pub timer: Rect,
    pub new_game: Rect,
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

/// Computes the layout of the whole screen. Pure function of the terminal
/// area and the game, so both rendering and event hit-testing use it.
pub fn layout(game: &Game) -> UiLayout {
    let (cols, rows) = game.size();
    let board = Rect::new(0, 1, (cols * 2) as u16, rows as u16);

    // Top bar elements, left to right: difficulty buttons, mine counter, timer, New Game.
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
    let mine_counter = Rect::new(x, 0, 9, 1);
    x += mine_counter.width;
    let timer = Rect::new(x, 0, 6, 1);
    x += timer.width;
    let new_game = Rect::new(x, 0, NEW_GAME.len() as u16, 1);

    UiLayout {
        difficulty_buttons,
        mine_counter,
        timer,
        new_game,
        board,
    }
}

/// Whether the terminal can fit the board and top bar.
pub fn fits(area: Rect, game: &Game) -> bool {
    let (cols, rows) = game.size();
    area.width >= (cols * 2) as u16 && area.height >= rows as u16 + 1
}

/// Renders the whole screen.
pub fn render(frame: &mut Frame, game: &Game) {
    let area = frame.area();
    let ui = layout(game);

    if !fits(area, game) {
        frame.render_widget(
            Paragraph::new("Terminal too small to fit the board")
                .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    render_top_bar(frame, &ui, game);
    render_board(frame, &ui, game);
}

fn render_top_bar(frame: &mut Frame, ui: &UiLayout, game: &Game) {
    for (difficulty, rect) in &ui.difficulty_buttons {
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
        Paragraph::new(format!("Mines:{:>3}", game.mines_remaining()))
            .style(Style::default().fg(Color::Yellow)),
        ui.mine_counter,
    );

    let secs = game.elapsed().as_secs();
    let timer = format!("{:02}:{:02}", secs / 60, secs % 60);
    frame.render_widget(
        Paragraph::new(timer).style(Style::default().fg(Color::Cyan)),
        ui.timer,
    );

    frame.render_widget(
        Paragraph::new(NEW_GAME).style(Style::default().fg(Color::Black).bg(Color::Blue)),
        ui.new_game,
    );

    // Result banner on the right side of the top bar, if the game has ended.
    let status = match game.game_state() {
        GameState::Won => "WON",
        GameState::Lost => "LOST",
        _ => "",
    };
    if !status.is_empty() {
        let color = if game.game_state() == GameState::Won {
            Color::Green
        } else {
            Color::Red
        };
        frame.render_widget(
            Paragraph::new(status).style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Rect::new(ui.new_game.right() + 2, 0, status.len() as u16, 1),
        );
    }
}

fn render_board(frame: &mut Frame, ui: &UiLayout, game: &Game) {
    let buf = frame.buffer_mut();
    let (cols, rows) = game.size();
    for row in 0..rows {
        for col in 0..cols {
            let x = ui.board.x + (col as u16) * 2;
            let y = ui.board.y + row as u16;
            let view = game.cell_state(Position::new(row, col));
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
                    &n.to_string(),
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

    if !fits(area, &app.game) {
        return;
    }

    let pos = TermPos::new(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(action) = hit_test(app.ui, pos) {
                match action {
                    Hit::NewGame => app.reset(),
                    Hit::Difficulty(d) => {
                        app.difficulty = d;
                        app.reset();
                    }
                }
            } else if let Some(pos) = board_cell(app.ui, pos) {
                if app.right_pressed && app.game.game_state() == GameState::Playing {
                    app.game.chord(pos);
                } else {
                    app.game.reveal(pos);
                }
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            app.right_pressed = true;
            if let Some(pos) = board_cell(app.ui, pos) {
                app.game.toggle_flag(pos);
            }
        }
        MouseEventKind::Up(MouseButton::Right) => {
            app.right_pressed = false;
        }
        _ => {}
    }
}

/// Handles keyboard events: only app-lifecycle keys (q / Ctrl+C) — the game
/// itself is mouse-only by design.
pub fn handle_key(key: ratatui::crossterm::event::KeyEvent) -> bool {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Char('q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}

enum Hit {
    NewGame,
    Difficulty(Difficulty),
}

fn hit_test(ui: UiLayout, pos: TermPos) -> Option<Hit> {
    if ui.new_game.contains(pos) {
        return Some(Hit::NewGame);
    }
    for (difficulty, rect) in ui.difficulty_buttons {
        if rect.contains(pos) {
            return Some(Hit::Difficulty(difficulty));
        }
    }
    None
}

/// Maps a terminal position to a board Cell, if inside the board area.
fn board_cell(ui: UiLayout, pos: TermPos) -> Option<Position> {
    if pos.x < ui.board.x || pos.y < ui.board.y {
        return None;
    }
    let col = ((pos.x - ui.board.x) / 2) as usize;
    let row = (pos.y - ui.board.y) as usize;
    Some(Position::new(row, col))
}

/// Application state: the game plus UI-only state.
pub struct App {
    pub game: Game,
    pub difficulty: Difficulty,
    pub right_pressed: bool,
    ui: UiLayout,
}

impl App {
    pub fn new() -> Self {
        let difficulty = Difficulty::Beginner;
        Self {
            game: Game::new(difficulty),
            difficulty,
            right_pressed: false,
            ui: UiLayout {
                difficulty_buttons: [(Difficulty::Beginner, Rect::ZERO); 3],
                mine_counter: Rect::ZERO,
                timer: Rect::ZERO,
                new_game: Rect::ZERO,
                board: Rect::ZERO,
            },
        }
    }

    pub fn update_layout(&mut self) {
        self.ui = layout(&self.game);
    }

    fn reset(&mut self) {
        self.game = Game::new(self.difficulty);
        self.right_pressed = false;
    }
}
