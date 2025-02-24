use color_eyre::Result;
use crossterm::terminal::disable_raw_mode;
use notemancy_core::config;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::Stdout;

/// Opens an arbitrary file in the default editor (using $EDITOR or "vi").
/// It restores the terminal, launches the editor for the given path, then reinitializes the terminal.
pub fn open_file_in_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    path: &str,
) -> Result<()> {
    // Restore terminal state so the editor can work.
    ratatui::restore();
    disable_raw_mode()?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let _ = std::process::Command::new(editor).arg(path).status();
    // Reinitialize terminal.
    *terminal = ratatui::init();
    Ok(())
}

/// (Your existing open_config_in_editor remains unchanged.)
pub fn open_config_in_editor(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    ratatui::restore();
    disable_raw_mode()?;
    if let Err(e) = config::open_config_in_editor() {
        eprintln!("Error opening config: {}", e);
    }
    *terminal = ratatui::init();
    Ok(())
}
