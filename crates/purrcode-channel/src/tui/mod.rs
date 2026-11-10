pub mod app;
pub mod render;

pub use app::{MessageItem, ToolStatus, TuiApp};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use purrcode_api::error::{PurrError, Result};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;

pub struct TuiRunner;

impl TuiRunner {
    pub fn run(mut app: TuiApp) -> Result<()> {
        enable_raw_mode().map_err(|e| PurrError::Channel {
            channel: "tui".to_string(),
            message: format!("Failed to enable raw mode: {e}"),
        })?;

        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen).map_err(|e| PurrError::Channel {
            channel: "tui".to_string(),
            message: format!("Failed to enter alternate screen: {e}"),
        })?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(|e| PurrError::Channel {
            channel: "tui".to_string(),
            message: format!("Failed to initialize terminal: {e}"),
        })?;

        let res = run_loop(&mut terminal, &mut app);

        // Always restore terminal on exit
        let _ = disable_raw_mode();
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
        let _ = terminal.show_cursor();

        res
    }
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut TuiApp,
) -> Result<()> {
    while !app.should_quit {
        terminal
            .draw(|f| render::render(f, app))
            .map_err(|e| PurrError::Channel {
                channel: "tui".to_string(),
                message: format!("Draw error: {e}"),
            })?;

        if event::poll(Duration::from_millis(50)).map_err(|e| PurrError::Channel {
            channel: "tui".to_string(),
            message: format!("Event poll error: {e}"),
        })? {
            if let Event::Key(key) = event::read().map_err(|e| PurrError::Channel {
                channel: "tui".to_string(),
                message: format!("Event read error: {e}"),
            })? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    app.should_quit = true;
                    break;
                }

                match key.code {
                    KeyCode::Char(c) => app.insert_char(c),
                    KeyCode::Backspace => app.backspace(),
                    KeyCode::Delete => app.delete(),
                    KeyCode::Left => app.move_cursor_left(),
                    KeyCode::Right => app.move_cursor_right(),
                    KeyCode::Up => {
                        if app.scroll_offset > 0 {
                            app.scroll_offset -= 1;
                        }
                    }
                    KeyCode::Down => {
                        app.scroll_offset += 1;
                    }
                    KeyCode::Enter => {
                        if let Some(cmd) = app.submit_input() {
                            if cmd == "/quit" || cmd == "/exit" {
                                app.should_quit = true;
                                break;
                            } else {
                                app.items.push(MessageItem::User(cmd.clone()));
                                app.items.push(MessageItem::Assistant {
                                    text: format!("Echo response for '{}' (TUI interactive hook). Use 'purrcode-ai -p' for live multi-turn agent streaming.", cmd),
                                    reasoning: None,
                                });
                            }
                        }
                    }
                    KeyCode::Esc => {
                        app.should_quit = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
