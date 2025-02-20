use color_eyre::eyre::Report;
use color_eyre::Result;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::Stylize;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use notemancy_core::scan::{ScannedFile, Scanner};
// Import the search API types.
use notemancy_core::search::{Document, SearchInterface};

use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

fn highlight_markdown(content: &str) -> Vec<Line> {
    let parser = Parser::new(content);
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();

    for event in parser {
        match event {
            MdEvent::Start(Tag::CodeBlock(info)) => {
                in_code_block = true;
                match info {
                    CodeBlockKind::Fenced(lang) => {
                        code_lang = lang.to_string().to_owned();
                    }
                    CodeBlockKind::Indented => {
                        code_lang.clear();
                    }
                }
            }
            MdEvent::End(Tag::CodeBlock(_)) => {
                let syntax = SYNTAX_SET
                    .find_syntax_by_token(code_lang.as_str())
                    .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
                let theme = &THEME_SET.themes["base16-ocean.dark"];
                let mut highlighter = HighlightLines::new(syntax, theme);
                // Collect lines so we don't hold a borrow on code_buffer.
                let code_lines: Vec<&str> = code_buffer.lines().collect();
                for line in code_lines {
                    let ranges = highlighter.highlight(line, &SYNTAX_SET);
                    let spans: Vec<Span> = ranges
                        .into_iter()
                        .map(|(s, text)| {
                            let fg = Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b);
                            Span::styled(text.to_string(), Style::default().fg(fg))
                        })
                        .collect();
                    lines.push(Line::from(spans));
                }
                code_buffer.clear();
                in_code_block = false;
            }
            MdEvent::Text(text) => {
                if in_code_block {
                    code_buffer.push_str(&text);
                } else {
                    for line in text.lines() {
                        lines.push(Line::from(line.to_string()));
                    }
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                if !in_code_block {
                    lines.push(Line::from(String::new()));
                }
            }
            _ => {}
        }
    }
    lines
}

#[derive(Debug, PartialEq)]
enum AppState {
    Starting,
    Scanning,
    Preview,
    Indexing,
    Search,
}

type ScanReceiver = Option<Receiver<Result<(Vec<ScannedFile>, String), Report>>>;
type IndexReceiver = Option<std::sync::mpsc::Receiver<()>>;

pub struct App {
    running: bool,
    state: AppState,
    spinner_idx: usize,
    spinner_chars: Vec<char>,
    scan_result: Option<Vec<ScannedFile>>,
    scan_summary: Option<String>,
    scanning_receiver: ScanReceiver,
    last_tick: Instant,
    // For search mode:
    search_query: String,
    search_results: Vec<Document>,
    selected_search_index: usize,
    // Store the search interface.
    search_interface: Option<Arc<SearchInterface>>,
    indexing_receiver: IndexReceiver,
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
            search_interface: None,
            indexing_receiver: None,
        }
    }
}

// Manually implement Debug for App, skipping non-Debug fields.
impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("running", &self.running)
            .field("state", &self.state)
            .field("spinner_idx", &self.spinner_idx)
            .field("spinner_chars", &self.spinner_chars)
            .field("scan_result", &self.scan_result)
            .field("scan_summary", &self.scan_summary)
            .field("last_tick", &self.last_tick)
            .field("search_query", &self.search_query)
            .field("search_results", &self.search_results)
            .field("selected_search_index", &self.selected_search_index)
            .finish()
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    /// Setter so the app can use a preconfigured SearchInterface.
    pub fn set_search_interface(&mut self, si: SearchInterface) {
        self.search_interface = Some(Arc::new(si));
    }

    // Notice the terminal type is now fixed:
    pub fn run(
        mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
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
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) {
        match self.state {
            AppState::Search => self.handle_search_key(key, terminal),
            _ => self.handle_default_key(key),
        }
    }

    fn handle_default_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => self.quit(),
            _ => {}
        }
    }

    /// Enters search mode by building the search index first.
    fn enter_search_mode(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) {
        let _ = terminal;
        self.state = AppState::Indexing;
        self.search_query.clear();
        self.search_results.clear();
        self.selected_search_index = 0;
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        self.indexing_receiver = Some(rx);

        let scanned_files = self.scan_result.clone();
        let search_interface = self.search_interface.as_ref().unwrap().clone();

        thread::spawn(move || {
            if let Some(scanned) = scanned_files {
                let file_paths: Vec<PathBuf> =
                    scanned.into_iter().map(|sf| sf.local_path).collect();
                let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
                if let Err(e) = rt.block_on(search_interface.index_files(file_paths)) {
                    eprintln!("Indexing error: {}", e);
                }
            }
            let _ = tx.send(());
        });
    }

    /// Performs a search using the configured search interface.
    fn perform_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
            self.selected_search_index = 0;
            return;
        }
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        if let Some(ref si) = self.search_interface {
            match rt.block_on(si.search(&self.search_query)) {
                Ok(docs) => {
                    self.search_results = docs;
                    self.selected_search_index = 0;
                }
                Err(e) => {
                    eprintln!("Search error: {}", e);
                    self.search_results.clear();
                }
            }
        } else {
            eprintln!("Search interface not configured!");
            self.search_results.clear();
        }
    }

    fn handle_search_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) {
        match key.code {
            KeyCode::Esc => {
                // Exit search mode and return to the preview.
                self.state = AppState::Preview;
            }
            KeyCode::Enter => {
                if let Some(doc) = self.search_results.get(self.selected_search_index) {
                    open_file_in_editor(terminal, &doc.path);
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

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        match self.state {
            AppState::Starting => {
                frame.render_widget(Paragraph::new("notemancy is starting"), area);
            }
            AppState::Scanning => {
                let spinner = self.spinner_chars[self.spinner_idx];
                let text = format!("Scanning... {}", spinner);
                frame.render_widget(Paragraph::new(text), area);
            }
            AppState::Indexing => {
                let spinner = self.spinner_chars[self.spinner_idx];
                let text = format!("Building search index... {}", spinner);
                frame.render_widget(Paragraph::new(text), area);
            }
            AppState::Preview => {
                let title = Line::from("Ratatui Simple Template")
                    .bold()
                    .blue()
                    .centered();
                let text = "Hello, Ratatui!\n\n\
                            Created using https://github.com/ratatui/templates\n\
                            Press Ctrl+S to search.\n\
                            Press `Esc`, `Ctrl-C` or `q` to stop running.";
                frame.render_widget(
                    Paragraph::new(text)
                        .block(Block::default().borders(Borders::ALL).title(title))
                        .centered(),
                    area,
                );
            }
            AppState::Search => self.draw_search_ui(frame, area),
        }
    }

    fn draw_search_ui(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(area);

        let input = Paragraph::new(self.search_query.as_str())
            .block(Block::default().borders(Borders::ALL).title("Search"));
        frame.render_widget(input, chunks[0]);

        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(chunks[1]);

        let items: Vec<ListItem> = self
            .search_results
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let style = if i == self.selected_search_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Span::styled(&doc.path, style))
            })
            .collect();

        let results_list =
            List::new(items).block(Block::default().borders(Borders::ALL).title("Results"));
        frame.render_widget(results_list, bottom_chunks[0]);

        if let Some(doc) = self.search_results.get(self.selected_search_index) {
            let highlighted = highlight_markdown(&doc.content);
            let preview = Paragraph::new(highlighted)
                .block(Block::default().borders(Borders::ALL).title("Preview"));
            frame.render_widget(preview, bottom_chunks[1]);
        } else {
            let preview = Paragraph::new("No file selected.")
                .block(Block::default().borders(Borders::ALL).title("Preview"));
            frame.render_widget(preview, bottom_chunks[1]);
        }
    }

    fn quit(&mut self) {
        self.running = false;
    }
}

/// Opens the specified file in the default editor (from $EDITOR, fallback "vi").
/// Temporarily restores the terminal, launches the editor, waits for exit,
/// then reinitializes the terminal.
fn open_file_in_editor(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    path: &str,
) {
    // Restore the terminal state.
    ratatui::restore();
    disable_raw_mode().expect("Failed to disable raw mode");
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let _ = std::process::Command::new(editor).arg(path).status();
    // Reinitialize the terminal.
    *terminal = ratatui::init();
}
