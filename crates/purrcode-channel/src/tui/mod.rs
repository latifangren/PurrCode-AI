pub mod app;
pub mod render;

pub use app::{ApprovalRequest, MessageItem, ToolStatus, TuiApp, TuiEvent};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use purrcode_api::error::{PurrError, Result};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

pub struct TuiRunner;

impl TuiRunner {
    pub fn run(mut app: TuiApp, mut rx: Option<Receiver<TuiEvent>>) -> Result<()> {
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

        let res = run_loop(&mut terminal, &mut app, rx.as_mut());

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
    mut rx: Option<&mut Receiver<TuiEvent>>,
) -> Result<()> {
    while !app.should_quit {
        // 1. Drain any pending events from the background agent loop
        if let Some(ref mut event_receiver) = rx {
            while let Ok(event) = event_receiver.try_recv() {
                app.handle_event(event);
            }
        }

        // 2. Render current frame
        terminal
            .draw(|f| render::render(f, app))
            .map_err(|e| PurrError::Channel {
                channel: "tui".to_string(),
                message: format!("Draw error: {e}"),
            })?;

        // 3. Poll keyboard events
        if event::poll(Duration::from_millis(30)).map_err(|e| PurrError::Channel {
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

                // If approval card is active, intercept [a], [t], [e]
                if let Some(req) = app.pending_approval.take() {
                    match key.code {
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            let _ = req.tx.send(true);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action approved by user.".to_string(),
                                success: true,
                            });
                            continue;
                        }
                        KeyCode::Char('t') | KeyCode::Char('T') | KeyCode::Esc => {
                            let _ = req.tx.send(false);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action rejected by user.".to_string(),
                                success: false,
                            });
                            continue;
                        }
                        KeyCode::Char('e') | KeyCode::Char('E') => {
                            let _ = req.tx.send(false);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action cancelled for manual edits.".to_string(),
                                success: false,
                            });
                            continue;
                        }
                        _ => {
                            // Put request back if other key pressed
                            app.pending_approval = Some(req);
                            continue;
                        }
                    }
                }

                // Normal editing keys
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
