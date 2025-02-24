pub mod app;
pub mod config_editor;

use app::core::App;
use color_eyre::eyre::Report;
use color_eyre::Result;
use notemancy_core::search::{MeiliSearchServer, SearchInterface};

fn main() -> Result<()> {
    // Install color-eyre for improved error reports.
    color_eyre::install()?;

    // Start the MeiliSearch server.
    let mut server = MeiliSearchServer::start().map_err(|e| Report::msg(e.to_string()))?;
    println!("MeiliSearch started on port {}", server.port);

    // Create the search interface using the same port.
    let search_interface =
        SearchInterface::new_from_server(&server).map_err(|e| Report::msg(e.to_string()))?;

    // Initialize the terminal using ratatui's helper.
    let mut terminal = ratatui::init();

    // Create the app and inject the search interface.
    let mut app = App::new();
    app.set_search_interface(search_interface);

    // Run the app.
    let result = app.run(&mut terminal);

    // Restore the terminal state.
    ratatui::restore();

    // Shutdown the MeiliSearch server.
    server.shutdown().map_err(|e| Report::msg(e.to_string()))?;

    result
}
