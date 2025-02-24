use crate::app::core::{App, AppState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::Stdout;

// Define a type alias for command actions.
pub type CommandAction = Box<dyn Fn(&mut App, &mut Terminal<CrosstermBackend<Stdout>>) + Send>;

pub struct CommandItem {
    pub name: &'static str,
    pub description: &'static str,
    pub action: CommandAction,
}

type Type = *const Box<
    dyn Fn(
            &mut crate::app::core::App,
            &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
        ) + Send,
>;

/// Handles key events when the command palette is active.
pub fn handle_command_palette_key(
    app: &mut App,
    key: KeyEvent,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) {
    match key.code {
        KeyCode::Esc => {
            app.state = AppState::Preview;
        }
        KeyCode::Up => {
            if app.selected_command_index > 0 {
                app.selected_command_index -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_command_index + 1 < app.command_items.len() {
                app.selected_command_index += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(cmd) = app.command_items.get(app.selected_command_index) {
                // Use a raw pointer to call the closure to avoid borrow conflicts.
                let action_ptr: Type = &cmd.action as *const _;
                unsafe {
                    (*action_ptr)(app, terminal);
                }
            }
        }
        _ => {}
    }
}
