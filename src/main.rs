pub mod app;
pub mod config_editor;

use app::core::App;
use color_eyre::eyre::Report;
use color_eyre::Result;
use notemancy_core::search::init_search_engine;

fn main() -> Result<()> {
    // Install color-eyre for improved error reports.
    color_eyre::install()?;

    // Initialize the search engine
    let search_engine = init_search_engine()
        .map_err(|e| Report::msg(format!("Failed to initialize search engine: {}", e)))?;
    println!("Search engine initialized successfully");

    // Initialize the terminal using ratatui's helper.
    let mut terminal = ratatui::init();

    // Create the app and inject the search engine.
    let mut app = App::new();
    app.set_search_engine(search_engine);

    // Run the app.
    let result = app.run(&mut terminal);

    // Restore the terminal state.
    ratatui::restore();

    result
}
