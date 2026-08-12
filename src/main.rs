mod core;
mod ui;

use std::io::stdout;
use std::time::Duration;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;

    let mut app = ui::App::new();
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
