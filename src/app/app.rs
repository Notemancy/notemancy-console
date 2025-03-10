use crate::app::command_palette::CommandItem;
use ratatui::widgets::Block;

use crate::app::ui::{draw_command_palette, draw_search_ui};
use crate::config_editor;
use color_eyre::eyre::Report;
use color_eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use notemancy_core::scan::{ScannedFile, Scanner};
use notemancy_core::search::{SearchEngine, SearchResult};
use ratatui::style::{Color, Style};
use std::{
    io::Stdout,
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

pub enum AppState {
    Starting,
    Scanning,
    Preview,
    Indexing,
    Search,
    CommandPalette,
}

type ScanReceiver = Option<Receiver<Result<(Vec<ScannedFile>, String), Report>>>;
type IndexReceiver = Option<std::sync::mpsc::Receiver<()>>;

pub struct App {
    pub running: bool,
    pub state: AppState,
    pub spinner_idx: usize,
    pub spinner_chars: Vec<char>,
    pub scan_result: Option<Vec<ScannedFile>>,
    pub scan_summary: Option<String>,
    pub scanning_receiver: ScanReceiver,
    pub last_tick: Instant,
    // For search mode:
    pub search_query: String,
    pub search_results: Vec<SearchResult>,
    pub selected_search_index: usize,
    // Store the search engine
    pub search_engine: Option<SearchEngine>,
    pub indexing_receiver: IndexReceiver,
    // Command palette fields:
    pub command_items: Vec<CommandItem>,
    pub selected_command_index: usize,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: false,
            state: AppState::Starting,
            spinner_idx: 0,
            spinner_chars: vec!['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            scan_result: None,
            scan_summary: None,
            scanning_receiver: None,
            last_tick: Instant::now(),
            search_query: String::new(),
            search_results: Vec::new(),
            selected_search_index: 0,
            search_engine: None,
            indexing_receiver: None,
            command_items: Vec::new(),
            selected_command_index: 0,
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    /// Setter for injecting a preconfigured SearchEngine.
    pub fn set_search_engine(&mut self, engine: SearchEngine) {
        self.search_engine = Some(engine);
    }

    pub fn enter_command_palette(&mut self) {
        // Populate the command list.
        self.command_items = vec![
            crate::app::command_palette::CommandItem {
                name: "Search",
                description: "Enter search mode",
                action: Box::new(|app, terminal| {
                    app.enter_search_mode(terminal);
                    app.state = AppState::Preview;
                }),
            },
            crate::app::command_palette::CommandItem {
                name: "Open Config Editor",
                description: "Edit configuration file",
                action: Box::new(|app, terminal| {
                    if let Err(e) = crate::config_editor::open_config_in_editor(terminal) {
                        eprintln!("Error opening config: {}", e);
                    }
                    app.state = AppState::Preview;
                }),
            },
            crate::app::command_palette::CommandItem {
                name: "Quit",
                description: "Exit the application",
                action: Box::new(|app, _terminal| {
                    app.quit();
                }),
            },
        ];
        self.selected_command_index = 0;
        self.state = AppState::CommandPalette;
    }

    /// Run the application.
    pub fn run(
        mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    ) -> Result<()> {
        enable_raw_mode()?;
        println!("notemancy is starting");

        let (tx, rx) = mpsc::channel::<Result<(Vec<ScannedFile>, String), Report>>();
        self.scanning_receiver = Some(rx);
        self.state = AppState::Scanning;

        thread::spawn(move || {
            let scanner = match Scanner::from_config().map_err(|e| Report::msg(e.to_string())) {
                Ok(s) => s,
                Err(e) => return tx.send(Err(e)).unwrap_or(()),
            };
            let res = scanner
                .scan_markdown_files()
                .map_err(|e| Report::msg(e.to_string()));
            tx.send(res).unwrap_or(());
        });

        self.running = true;
        while self.running {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('s')
                        {
                            self.enter_search_mode(terminal);
                        } else if key.modifiers.contains(KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('e')
                        {
                            if let Err(e) = config_editor::open_config_in_editor(terminal) {
                                eprintln!("Error opening config: {}", e);
                            }
                            continue;
                        } else {
                            self.handle_key(key, terminal);
                        }
                    }
                }
            } else if self.last_tick.elapsed() >= Duration::from_millis(100) {
                self.spinner_idx = (self.spinner_idx + 1) % self.spinner_chars.len();
                self.last_tick = Instant::now();
            }

            if let Some(ref rx) = self.scanning_receiver {
                match rx.try_recv() {
                    Ok(result) => {
                        match result {
                            Ok((scanned_files, summary)) => {
                                self.scan_result = Some(scanned_files);
                                self.scan_summary = Some(summary);
                            }
                            Err(e) => eprintln!("Scanning error: {}", e),
                        }
                        self.state = AppState::Preview;
                        self.scanning_receiver = None;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        self.scanning_receiver = None;
                    }
                }
            }

            if let Some(ref rx) = self.indexing_receiver {
                if rx.try_recv().is_ok() {
                    self.state = AppState::Search;
                    self.indexing_receiver = None;
                }
            }

            terminal.draw(|frame| self.draw(frame))?;
        }

        disable_raw_mode()?;
        Ok(())
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    ) {
        match self.state {
            AppState::Search => self.handle_search_key(key, terminal),
            AppState::CommandPalette => self.handle_command_palette_key(key, terminal),
            _ => self.handle_default_key(key),
        }
    }

    fn handle_default_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => self.quit(),
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => self.enter_command_palette(),
            _ => {}
        }
    }

    // Delegate command palette key events to a helper in the ui module.
    fn handle_command_palette_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    ) {
        // The implementation is now moved to the command_palette module.
        crate::app::command_palette::handle_command_palette_key(self, key, terminal);
    }

    pub fn enter_search_mode(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    ) {
        let _ = terminal;
        self.state = AppState::Indexing;
        self.search_query.clear();
        self.search_results.clear();
        self.selected_search_index = 0;
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        self.indexing_receiver = Some(rx);

        // Import Database for indexing
        use notemancy_core::db::Database;

        // Clone the search engine for the indexing thread
        if let Some(ref search_engine) = self.search_engine.clone() {
            let search_engine_clone = search_engine.clone();

            thread::spawn(move || {
                // Instead of indexing specific files, we'll use the database to get all files
                if let Ok(db) = Database::new() {
                    if let Err(e) = search_engine_clone.index_all_documents(&db) {
                        eprintln!("Indexing error: {}", e);
                    }
                } else {
                    eprintln!("Failed to connect to database");
                }

                let _ = tx.send(());
            });
        } else {
            eprintln!("Search engine not configured!");
            // Skip indexing and move directly to search
            let _ = tx.send(());
        }
    }

    fn perform_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
            self.selected_search_index = 0;
            return;
        }

        if let Some(ref search_engine) = self.search_engine {
            match search_engine.search(&self.search_query, 20) {
                Ok(results) => {
                    self.search_results = results;
                    self.selected_search_index = 0;
                }
                Err(e) => {
                    eprintln!("Search error: {}", e);
                    self.search_results.clear();
                }
            }
        } else {
            eprintln!("Search engine not configured!");
            self.search_results.clear();
        }
    }

    fn handle_search_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    ) {
        match key.code {
            KeyCode::Esc => {
                self.state = AppState::Preview;
            }
            KeyCode::Enter => {
                if let Some(doc) = self.search_results.get(self.selected_search_index) {
                    let _ = crate::config_editor::open_file_in_editor(terminal, &doc.path);
                    self.state = AppState::Preview;
                }
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.perform_search();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.perform_search();
            }
            KeyCode::Up => {
                if self.selected_search_index > 0 {
                    self.selected_search_index -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected_search_index + 1 < self.search_results.len() {
                    self.selected_search_index += 1;
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        match self.state {
            AppState::Starting => {
                let paragraph = ratatui::widgets::Paragraph::new("notemancy is starting")
                    .style(
                        Style::default()
                            .fg(Color::Rgb(224, 224, 224))
                            .bg(Color::Rgb(22, 22, 22)),
                    )
                    .block(Block::default());
                frame.render_widget(paragraph, area);
            }
            AppState::Scanning => {
                let spinner = self.spinner_chars[self.spinner_idx];
                let text = format!("Scanning... {}", spinner);
                let paragraph = ratatui::widgets::Paragraph::new(text)
                    .style(
                        Style::default()
                            .fg(Color::Rgb(224, 224, 224))
                            .bg(Color::Rgb(22, 22, 22)),
                    )
                    .block(Block::default());
                frame.render_widget(paragraph, area);
            }
            AppState::Indexing => {
                let spinner = self.spinner_chars[self.spinner_idx];
                let text = format!("Building search index... {}", spinner);
                let paragraph = ratatui::widgets::Paragraph::new(text)
                    .style(
                        Style::default()
                            .fg(Color::Rgb(224, 224, 224))
                            .bg(Color::Rgb(22, 22, 22)),
                    )
                    .block(Block::default());
                frame.render_widget(paragraph, area);
            }
            AppState::Preview => {
                let text = "Hello, Ratatui!\n\nCreated using https://github.com/ratatui/templates\nPress Ctrl+S to search.\nPress Ctrl+P for commands.\nPress Esc, Ctrl-C or q to quit.";
                let paragraph = ratatui::widgets::Paragraph::new(text)
                    .style(
                        Style::default()
                            .fg(Color::Rgb(224, 224, 224))
                            .bg(Color::Rgb(22, 22, 22)),
                    )
                    .alignment(ratatui::layout::Alignment::Center)
                    .block(Block::default());
                frame.render_widget(paragraph, area);
            }
            AppState::Search => {
                draw_search_ui(self, frame);
            }
            AppState::CommandPalette => {
                draw_command_palette(self, frame, area);
            }
        }
    }

    fn quit(&mut self) {
        self.running = false;
    }
}
