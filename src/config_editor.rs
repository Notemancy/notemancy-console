// src/config_editor.rs

use color_eyre::Result;
use crossterm::terminal::disable_raw_mode;
use notemancy_core::config;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::Stdout;

/// Opens the configuration file in the default editor defined by the SHELL.
/// It restores the terminal, calls the notemancy_core config API to open the config,
/// then reinitializes the TUI terminal.
pub fn open_config_in_editor(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    // Restore the terminal so the editor can take over.
    ratatui::restore();
    disable_raw_mode()?;

    // Use the config module from notemancy-core.
    if let Err(e) = config::open_config_in_editor() {
        eprintln!("Error opening config: {}", e);
    }

    // Reinitialize the terminal.
    *terminal = ratatui::init();
    Ok(())
}
