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
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetailViewMode {
    Preview,
    RelatedFiles,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,  // Navigation mode where shortcuts work
    Editing, // Text input mode for the search query
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppState {
    Starting,
    Scanning,
    Preview,
    Indexing,
    Search,
    CommandPalette,
    IndexingVectors,
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
    // Store the search interface.
    pub search_engine: Option<SearchEngine>,
    pub indexing_receiver: IndexReceiver,
    pub detail_view_mode: DetailViewMode,
    pub related_files: Vec<SearchResult>,
    pub input_mode: InputMode,

    pub is_loading_related_files: bool,
    pub related_files_receiver: Option<
        std::sync::mpsc::Receiver<Result<Vec<notemancy_core::search::SearchResult>, String>>,
    >,
    pub related_files_error: Option<String>,
    // Command palette fields:
    pub command_items: Vec<CommandItem>,
    pub selected_command_index: usize,
    pub vector_indexing_status: Option<String>, // To display status messages during vector indexing
    pub vector_indexing_complete: bool,         // Flag to indicate when indexing is complete
    pub vector_indexing_success_time: Option<Instant>,
    pub vector_indexing_receiver: Option<std::sync::mpsc::Receiver<String>>,
    pub last_selected_index: usize,
    pub last_selection_change: std::time::Instant,
    pub debounce_duration: std::time::Duration,
    pub current_related_document_path: Option<String>,
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
            vector_indexing_status: None,
            vector_indexing_complete: false,
            vector_indexing_success_time: None,
            vector_indexing_receiver: None,
            detail_view_mode: DetailViewMode::Preview,
            related_files: Vec::new(),
            input_mode: InputMode::Editing,
            is_loading_related_files: false,
            related_files_receiver: None,
            related_files_error: None,
            last_selected_index: 0,
            last_selection_change: Instant::now(),
            debounce_duration: Duration::from_millis(1000), // 500ms debounce
            current_related_document_path: None,
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_search_engine(&mut self, engine: SearchEngine) {
        self.search_engine = Some(engine);
    }

    pub fn enter_vector_indexing_mode(&mut self) {
        self.state = AppState::IndexingVectors;
        self.vector_indexing_status = Some("Starting vector indexing...".to_string());
        self.vector_indexing_complete = false;
        self.vector_indexing_success_time = None;

        // Import required types from notemancy_core
        use notemancy_core::ai::AI;
        use notemancy_core::config::load_config;

        // Create a channel to communicate status updates
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        self.vector_indexing_receiver = Some(rx);

        // Create a thread to handle the indexing
        std::thread::spawn(move || {
            // Initialize runtime for async operations
            let rt = tokio::runtime::Runtime::new().unwrap();

            // Run the indexing process
            rt.block_on(async {
                // First load the configuration
                match load_config() {
                    Ok(config) => {
                        // Create the AI instance with config
                        match AI::new(&config).await {
                            Ok(ai) => {
                                let _ = tx.send("Processing documents...".to_string());

                                // Use the correct module name: vec_indexer
                                match notemancy_core::vec_indexer::index_markdown_files(&ai).await {
                                    Ok(_) => {
                                        let _ = tx.send("SUCCESS".to_string());
                                    }
                                    Err(e) => {
                                        let _ = tx.send(format!("Error: {}", e));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(format!("Error initializing AI: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(format!("Error loading config: {}", e));
                    }
                }
            });
        });
    }

    pub fn enter_command_palette(&mut self) {
        self.command_items = vec![
            crate::app::command_palette::CommandItem {
                name: "Search",
                description: "Enter search mode",
                action: Box::new(|app, terminal| {
                    app.enter_search_mode(terminal);
                    app.state = AppState::Search;
                }),
            },
            crate::app::command_palette::CommandItem {
                name: "Index Vectors",
                description: "Generate vector embeddings for all markdown files",
                action: Box::new(|app, _terminal| {
                    app.enter_vector_indexing_mode();
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

            if self.state == AppState::IndexingVectors {
                if let Some(ref rx) = self.vector_indexing_receiver {
                    match rx.try_recv() {
                        Ok(status) => {
                            if status == "SUCCESS" {
                                self.vector_indexing_status =
                                    Some("Vector indexing completed successfully!".to_string());
                                self.vector_indexing_complete = true;
                                self.vector_indexing_success_time = Some(Instant::now());
                            } else if status.starts_with("Error") {
                                self.vector_indexing_status = Some(status);
                                self.vector_indexing_complete = true;
                                self.vector_indexing_success_time = Some(Instant::now());
                            } else {
                                self.vector_indexing_status = Some(status);
                            }
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => {
                            self.vector_indexing_receiver = None;
                            self.state = AppState::Preview;
                        }
                    }
                }

                // Check if we need to return to the Preview state after showing success
                if let Some(success_time) = self.vector_indexing_success_time {
                    if success_time.elapsed() >= Duration::from_secs(2) {
                        self.state = AppState::Preview;
                        self.vector_indexing_success_time = None;
                    }
                }
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

            // self.process_related_files_receiver();
            self.process();

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

        // Create a channel to communicate when indexing is done
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        self.indexing_receiver = Some(rx);

        // Create a separate thread to handle indexing
        thread::spawn(move || {
            // Initialize the database
            match notemancy_core::db::Database::new() {
                Ok(db) => {
                    // Initialize a new search engine instance in this thread
                    match notemancy_core::search::init_search_engine() {
                        Ok(engine) => {
                            // Index all documents from the database
                            if let Err(e) = engine.index_all_documents(&db) {
                                eprintln!("Indexing error: {}", e);
                            }
                        }
                        Err(e) => eprintln!("Failed to initialize search engine: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to connect to database: {}", e),
            }

            // Signal that indexing is complete
            let _ = tx.send(());
        });
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
        match self.input_mode {
            InputMode::Normal => {
                match key.code {
                    // In Normal mode, handle navigation and view toggling
                    KeyCode::Esc => {
                        self.state = AppState::Preview;
                    }
                    KeyCode::Enter => {
                        if let Some(doc) = self.search_results.get(self.selected_search_index) {
                            let _ = crate::config_editor::open_file_in_editor(terminal, &doc.path);
                            self.state = AppState::Preview;
                        }
                    }
                    KeyCode::Tab | KeyCode::Char('r') => {
                        // Toggle between Preview and RelatedFiles modes
                        self.detail_view_mode = match self.detail_view_mode {
                            DetailViewMode::Preview => {
                                // When switching to RelatedFiles, we fetch the related files immediately
                                DetailViewMode::RelatedFiles
                            }
                            DetailViewMode::RelatedFiles => DetailViewMode::Preview,
                        };
                    
                        // If we just switched to RelatedFiles, update immediately
                        if self.detail_view_mode == DetailViewMode::RelatedFiles && !self.is_loading_related_files {
                            // Clear any existing related files
                            self.related_files.clear();
                            self.related_files_error = None;
                        
                            // Set last selected index to current so we don't trigger again on the same item
                            self.last_selected_index = self.selected_search_index;
                        
                            // Update immediately
                            self.get_related_files_for_selected();
                        }
                    }
                    KeyCode::Char('/') => {
                        // Enter editing mode with '/'
                        self.input_mode = InputMode::Editing;
                    }
                    KeyCode::Up => {
                        if self.selected_search_index > 0 {
                            let old_selection = self.selected_search_index;
                            self.selected_search_index -= 1;
                        
                            // Only mark as changed if actually changed
                            if old_selection != self.selected_search_index 
                              && self.detail_view_mode == DetailViewMode::RelatedFiles {
                                self.last_selection_change = Instant::now();
                            }
                        }
                    }
                    KeyCode::Down => {
                        if self.selected_search_index + 1 < self.search_results.len() {
                            let old_selection = self.selected_search_index;
                            self.selected_search_index += 1;
                        
                            // Only mark as changed if actually changed
                            if old_selection != self.selected_search_index 
                              && self.detail_view_mode == DetailViewMode::RelatedFiles {
                                self.last_selection_change = Instant::now();
                            }
                        }
                    }
                    _ => {}
                }
            }
            InputMode::Editing => {
                match key.code {
                    KeyCode::Esc => {
                        // Exit editing mode with Escape
                        self.input_mode = InputMode::Normal;
                    }
                    KeyCode::Enter => {
                        // Perform search and exit editing mode
                        self.perform_search();
                        self.input_mode = InputMode::Normal;
                    }
                    KeyCode::Char(c) => {
                        // Add character to search query while in editing mode
                        self.search_query.push(c);
                        self.perform_search();
                    }
                    KeyCode::Backspace => {
                        // Delete character from search query
                        self.search_query.pop();
                        self.perform_search();
                    }
                    _ => {}
                }
            }
        }
    }

    fn get_related_files_for_selected(&mut self) {
    // Don't do anything if we're already loading
    if self.is_loading_related_files {
        return;
    }
    
    // Clear existing related files
    self.related_files.clear();
    self.is_loading_related_files = true;
    self.related_files_error = None;

    // If we have a selected search result, find related files for it
    if let Some(selected_result) = self.search_results.get(self.selected_search_index) {
        // Create a channel to communicate results
        let (tx, rx) = std::sync::mpsc::channel();
        self.related_files_receiver = Some(rx);

        // Clone the path to use in the thread
        let path = selected_result.path.clone();

        // Spawn a thread to handle the async operation
        std::thread::spawn(move || {
            // Initialize runtime for async operations
            let rt = tokio::runtime::Runtime::new().unwrap();

            // Run the async process
            rt.block_on(async {
                // Load configuration
                match notemancy_core::config::load_config() {
                    Ok(config) => {
                        // Create AI instance
                        match notemancy_core::ai::AI::new(&config).await {
                            Ok(ai) => {
                                // First, read the content of the file to use for similarity search
                                let content = match std::fs::read_to_string(&path) {
                                    Ok(content) => content,
                                    Err(e) => {
                                        let _ = tx.send(Err(format!("Could not read file: {}", e)));
                                        return;
                                    }
                                };
                                
                                // Use the content directly with find_similar_documents API
                                // This ensures we're comparing based on content and not just paths
                                match ai.find_similar_documents(&content, 20, None).await {
                                    Ok(similar_docs) => {
                                        if similar_docs.is_empty() {
                                            let _ = tx.send(Err("No similar documents found.".to_string()));
                                            return;
                                        }
                                    
                                        // Process the results into SearchResult format
                                        let mut results = Vec::new();
                                    
                                        for (doc, score) in similar_docs {
                                            // Skip if no physical path in metadata
                                            let Some(rel_path) = doc.metadata.get("physical_path") else {
                                                continue;
                                            };
                                        
                                            // Skip the current document
                                            if rel_path == &path {
                                                continue;
                                            }
                                        
                                            // Extract title from path
                                            let title = std::path::Path::new(rel_path)
                                                .file_stem()
                                                .and_then(|s| s.to_str())
                                                .unwrap_or("")
                                                .to_string();
                                        
                                            // Convert score (0 is best, 1 is worst in distance metrics)
                                            // to a similarity percentage (100% is best, 0% is worst)
                                            let similarity = (1.0 - score) * 100.0;
                                        
                                            results.push(notemancy_core::search::SearchResult {
                                                path: rel_path.clone(),
                                                title,
                                                snippet: format!("Similarity: {:.1}%", similarity).into(),
                                                score: 1.0 - score, // Higher score = better match in SearchResult
                                            });
                                        }
                                    
                                        if results.is_empty() {
                                            let _ = tx.send(Err("No related documents found (current document excluded).".to_string()));
                                            return;
                                        }
                                    
                                        // Sort by score (highest first)
                                        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                                    
                                        // Take top 10
                                        let top_results = results.into_iter().take(10).collect();
                                    
                                        // Send results
                                        let _ = tx.send(Ok(top_results));
                                    },
                                    Err(e) => {
                                        let _ = tx.send(Err(format!("Error finding similar documents: {}", e)));
                                    }
                                }
                            },
                            Err(e) => {
                                let _ = tx.send(Err(format!("Error initializing AI: {}", e)));
                            }
                        }
                    },
                    Err(e) => {
                        let _ = tx.send(Err(format!("Error loading config: {}", e)));
                    }
                }
            });
        });
    } else {
        // No selected item, immediately clear loading state
        self.is_loading_related_files = false;
    }
}

    pub fn process(&mut self) {
    // Only do this for search mode in related files view
    if self.state == AppState::Search && 
       self.detail_view_mode == DetailViewMode::RelatedFiles && 
       !self.is_loading_related_files && 
       !self.search_results.is_empty() {
        
        // First, collect all the information we need without holding references
        let should_load = if let Some(selected_result) = self.search_results.get(self.selected_search_index) {
            let current_path = selected_result.path.clone();
            
            // Check if we already have related files for this document
            match &self.current_related_document_path {
                // If we haven't loaded related files for any document yet
                None => {
                    // We should load related files for the current document
                    Some(current_path)
                },
                // If we've loaded related files before
                Some(loaded_path) => {
                    // Only load if the path has changed (different document selected)
                    if loaded_path != &current_path {
                        Some(current_path)
                    } else {
                        None
                    }
                }
            }
        } else {
            None
        };
        
        // Now perform the action based on what we determined
        if let Some(path) = should_load {
            // Load related files for the new document
            self.get_related_files_for_selected();
            // Update which document we loaded for
            self.current_related_document_path = Some(path);
        }
    }
    
    // Process any completed related files requests
    self.process_related_files_receiver();
}



    pub fn process_related_files_receiver(&mut self) {
        if let Some(ref rx) = self.related_files_receiver {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(results) => {
                            if results.is_empty() {
                                self.related_files_error = Some("No related documents found that meet the similarity threshold.".to_string());
                            } else {
                                self.related_files = results;
                                self.related_files_error = None;
                            }
                        }
                        Err(error_msg) => {
                            self.related_files_error = Some(error_msg);
                        }
                    }
                    self.is_loading_related_files = false;
                    self.related_files_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Still waiting for results
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Channel closed without sending results
                    self.is_loading_related_files = false;
                    self.related_files_receiver = None;
                    self.related_files_error =
                        Some("Failed to process related files request".to_string());
                }
            }
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
            AppState::IndexingVectors => {
                crate::app::ui::draw_vector_indexing_ui(self, frame, area);
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
