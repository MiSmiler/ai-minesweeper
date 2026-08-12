mod core;
mod ui;

use std::io::stdout;
use std::time::Duration;

use clap::Parser;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;

use crate::core::GameMode;

/// Command-line options for the game.
#[derive(Parser)]
#[command(
    about = "A terminal Minesweeper game rendered with ratatui, controlled entirely by mouse."
)]
struct Cli {
    /// Prank Mode: the First Click of every game is always a Mine.
    #[arg(long)]
    prank: bool,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    let mode = if cli.prank {
        GameMode::Prank
    } else {
        GameMode::Classic
    };

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;

    let mut app = ui::App::new(mode);
    loop {
        terminal.draw(|frame| {
            app.update_layout();
            ui::render(frame, &app.game);
        })?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Mouse(mouse) => {
                    let area: Rect = terminal.size()?.into();
                    ui::handle_mouse(&mut app, mouse, area);
                }
                Event::Key(key) => {
                    if ui::should_quit(key) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    execute!(stdout(), DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}
