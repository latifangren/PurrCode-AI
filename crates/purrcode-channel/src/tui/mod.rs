// SPDX-License-Identifier: GPL-3.0-or-later

pub mod app;
pub mod render;

pub use app::{
    ApprovalRequest, AskOptionItem, AskPromptRequest, MessageItem, ToolStatus, TuiApp, TuiEvent,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use purrcode_api::error::{PurrError, Result};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

pub struct TuiRunner;

impl TuiRunner {
    pub fn run(
        app: TuiApp,
        rx: Option<Receiver<TuiEvent>>,
        prompt_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> Result<()> {
        Self::run_with_controls(app, rx, prompt_tx, None, None)
    }

    pub fn run_with_controls(
        mut app: TuiApp,
        mut rx: Option<Receiver<TuiEvent>>,
        prompt_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        steer_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
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

        let res = run_loop(
            &mut terminal,
            &mut app,
            rx.as_mut(),
            prompt_tx.as_ref(),
            steer_tx.as_ref(),
            cancel_flag.as_ref(),
        );

        // Always restore terminal on exit
        let _ = disable_raw_mode();
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
        let _ = terminal.show_cursor();

        res
    }
}

/// What a key press means while the approval card is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalKeyAction {
    Approve,
    /// Approve this action AND auto-approve every later approval prompt for
    /// the rest of this TUI session (hard policy denials still apply — the
    /// flag only skips the prompt, it cannot override a gate denial).
    ApproveAlways,
    Reject,
    CancelForEdit,
}

/// Map a key press to its approval-card action: `Some(Approve)` for
/// `a`/`y`/`Y`/`Enter`, `Some(ApproveAlways)` for `A` (capital A),
/// `Some(Reject)` for `t`/`T`/`n`/`N`/`Esc`,
/// `Some(CancelForEdit)` for `e`/`E`, `None` for any other key (the card
/// stays open). Pure function, covered by unit tests.
fn approval_key_action(code: KeyCode) -> Option<ApprovalKeyAction> {
    match code {
        KeyCode::Char('a') | KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            Some(ApprovalKeyAction::Approve)
        }
        KeyCode::Char('A') => Some(ApprovalKeyAction::ApproveAlways),
        KeyCode::Char('t')
        | KeyCode::Char('T')
        | KeyCode::Char('n')
        | KeyCode::Char('N')
        | KeyCode::Esc => Some(ApprovalKeyAction::Reject),
        KeyCode::Char('e') | KeyCode::Char('E') => Some(ApprovalKeyAction::CancelForEdit),
        _ => None,
    }
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut TuiApp,
    mut rx: Option<&mut Receiver<TuiEvent>>,
    prompt_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
    steer_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
    cancel_flag: Option<&Arc<AtomicBool>>,
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
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('d')
                    && app.input.is_empty()
                {
                    app.should_quit = true;
                    break;
                }

                // If approval card is active, intercept [a]/[y]/Enter, [t]/[n]/Esc, [e]
                if let Some(req) = app.pending_approval.take() {
                    match approval_key_action(key.code) {
                        Some(ApprovalKeyAction::Approve) => {
                            let _ = req.tx.send(true);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action approved by user.".to_string(),
                                success: true,
                            });
                            continue;
                        }
                        Some(ApprovalKeyAction::ApproveAlways) => {
                            req.always.store(true, Ordering::SeqCst);
                            let _ = req.tx.send(true);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action approved by user; approval prompts disabled for the rest of this session (hard policy rules still apply)."
                                    .to_string(),
                                success: true,
                            });
                            continue;
                        }
                        Some(ApprovalKeyAction::Reject) => {
                            let _ = req.tx.send(false);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action rejected by user.".to_string(),
                                success: false,
                            });
                            continue;
                        }
                        Some(ApprovalKeyAction::CancelForEdit) => {
                            let _ = req.tx.send(false);
                            app.items.push(MessageItem::ToolResult {
                                output: "Action cancelled for manual edits.".to_string(),
                                success: false,
                            });
                            continue;
                        }
                        None => {
                            app.pending_approval = Some(req);
                            continue;
                        }
                    }
                }

                // If Ask picker is active, intercept Up/Down/Enter/Esc/1..9
                if let Some(mut ask_req) = app.pending_ask.take() {
                    match key.code {
                        KeyCode::Up => {
                            if ask_req.selected_idx > 0 {
                                ask_req.selected_idx -= 1;
                            }
                            app.pending_ask = Some(ask_req);
                            continue;
                        }
                        KeyCode::Down => {
                            if ask_req.selected_idx + 1 < ask_req.options.len() {
                                ask_req.selected_idx += 1;
                            }
                            app.pending_ask = Some(ask_req);
                            continue;
                        }
                        KeyCode::Enter => {
                            let chosen = ask_req
                                .options
                                .get(ask_req.selected_idx)
                                .map(|o| o.id.clone());
                            let _ = ask_req.tx.send(chosen);
                            continue;
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let idx = (c as usize).saturating_sub('1' as usize);
                            if let Some(opt) = ask_req.options.get(idx) {
                                let _ = ask_req.tx.send(Some(opt.id.clone()));
                            } else {
                                app.pending_ask = Some(ask_req);
                            }
                            continue;
                        }
                        KeyCode::Esc => {
                            let _ = ask_req.tx.send(None);
                            continue;
                        }
                        _ => {
                            app.pending_ask = Some(ask_req);
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
                            if handle_slash_command(app, &cmd, prompt_tx) {
                                if app.should_quit {
                                    break;
                                }
                                continue;
                            }
                            if app.is_generating {
                                // Mid-turn follow-up message goes into SteeringQueue
                                app.items
                                    .push(MessageItem::User(format!("[steered] {}", cmd)));
                                if let Some(s_tx) = steer_tx {
                                    let _ = s_tx.send(cmd);
                                }
                            } else {
                                app.items.push(MessageItem::User(cmd.clone()));
                                app.is_generating = true;
                                if let Some(cf) = cancel_flag {
                                    cf.store(false, Ordering::SeqCst);
                                }
                                if let Some(p_tx) = prompt_tx {
                                    let _ = p_tx.send(cmd);
                                } else {
                                    app.items.push(MessageItem::Assistant {
                                        text: format!(
                                            "Echo response for '{}' (TUI standalone mode).",
                                            cmd
                                        ),
                                        reasoning: None,
                                    });
                                    app.is_generating = false;
                                }
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if app.is_generating {
                            if let Some(cf) = cancel_flag {
                                cf.store(true, Ordering::SeqCst);
                            }
                            app.is_generating = false;
                            app.items.push(MessageItem::ToolResult {
                                output: "Turn interrupted by user (Esc).".to_string(),
                                success: false,
                            });
                        } else {
                            app.should_quit = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn handle_slash_command(
    app: &mut TuiApp,
    cmd: &str,
    prompt_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
) -> bool {
    if !cmd.starts_with('/') {
        return false;
    }

    let mut parts = cmd.split_whitespace();
    let head = parts.next().unwrap_or("");
    let rest: String = parts.collect::<Vec<_>>().join(" ");

    match head {
        "/quit" | "/exit" => {
            app.should_quit = true;
            true
        }
        "/clear" => {
            app.items.clear();
            app.scroll_offset = 0;
            true
        }
        "/new" => {
            app.items.clear();
            app.scroll_offset = 0;
            app.session_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);
            if let Some(tx) = prompt_tx {
                let _ = tx.send("/__internal_reset".to_string());
            }
            app.items.push(MessageItem::Assistant {
                text: format!("Started new session: {}", app.session_id),
                reasoning: None,
            });
            true
        }
        "/help" | "/?" => {
            app.items.push(MessageItem::Assistant {
                text: "Slash commands: /help /new /resume <id> /fork /compact [focus] /clear /memory /export [jsonl|html] /stats /model <provider:model> /preset [name] /model-preset [name] /skill <name> /plugin list /status /quit".to_string(),
                reasoning: None,
            });
            true
        }
        "/status" | "/stats" => {
            app.items.push(MessageItem::Assistant {
                text: format!(
                    "Session: {} | Model: {} | Preset: {} | Model-Preset: {} | Tool Calls: {} | Tokens: {}",
                    app.session_id,
                    app.model_name,
                    app.preset_name,
                    app.model_preset_name,
                    app.tool_call_count,
                    app.token_usage
                ),
                reasoning: None,
            });
            true
        }
        "/preset" => {
            if !rest.is_empty() {
                app.preset_name = rest.clone();
                app.items.push(MessageItem::Assistant {
                    text: format!("Switched session mode preset to '{}'.", rest),
                    reasoning: None,
                });
            } else {
                app.items.push(MessageItem::Assistant {
                    text: format!(
                        "Active mode preset: {} (available: carrier, tabby, oyen)",
                        app.preset_name
                    ),
                    reasoning: None,
                });
            }
            true
        }
        "/model-preset" => {
            if !rest.is_empty() {
                app.model_preset_name = rest.clone();
                app.items.push(MessageItem::Assistant {
                    text: format!("Switched session model preset to '{}'.", rest),
                    reasoning: None,
                });
            } else {
                app.items.push(MessageItem::Assistant {
                    text: format!(
                        "Active model preset: {} (available: free, local, premium, custom-endpoint)",
                        app.model_preset_name
                    ),
                    reasoning: None,
                });
            }
            true
        }
        "/model" => {
            if !rest.is_empty() {
                app.model_name = rest.clone();
                app.items.push(MessageItem::Assistant {
                    text: format!("Active model updated to '{}'.", rest),
                    reasoning: None,
                });
            } else {
                app.items.push(MessageItem::Assistant {
                    text: format!("Current model: {}", app.model_name),
                    reasoning: None,
                });
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_key_action_approve_keys() {
        for code in [
            KeyCode::Char('a'),
            KeyCode::Char('y'),
            KeyCode::Char('Y'),
            KeyCode::Enter,
        ] {
            assert_eq!(
                approval_key_action(code),
                Some(ApprovalKeyAction::Approve),
                "key {code:?} must approve"
            );
        }
    }

    #[test]
    fn test_approval_key_action_approve_always_key() {
        // Capital `A` approves this action AND disables later prompts for
        // the rest of the session; lowercase `a` is a one-off approval.
        assert_eq!(
            approval_key_action(KeyCode::Char('A')),
            Some(ApprovalKeyAction::ApproveAlways)
        );
        assert_eq!(
            approval_key_action(KeyCode::Char('a')),
            Some(ApprovalKeyAction::Approve)
        );
    }

    #[test]
    fn test_approval_key_action_reject_keys() {
        for code in [
            KeyCode::Char('t'),
            KeyCode::Char('T'),
            KeyCode::Char('n'),
            KeyCode::Char('N'),
            KeyCode::Esc,
        ] {
            assert_eq!(
                approval_key_action(code),
                Some(ApprovalKeyAction::Reject),
                "key {code:?} must reject"
            );
        }
    }

    #[test]
    fn test_approval_key_action_edit_keys() {
        assert_eq!(
            approval_key_action(KeyCode::Char('e')),
            Some(ApprovalKeyAction::CancelForEdit)
        );
        assert_eq!(
            approval_key_action(KeyCode::Char('E')),
            Some(ApprovalKeyAction::CancelForEdit)
        );
    }

    #[test]
    fn test_approval_key_action_other_keys_ignored() {
        for code in [
            KeyCode::Char('q'),
            KeyCode::Char(' '),
            KeyCode::Up,
            KeyCode::Down,
        ] {
            assert_eq!(
                approval_key_action(code),
                None,
                "key {code:?} must be ignored"
            );
        }
    }
}
