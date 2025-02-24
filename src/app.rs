use crate::config_editor;
use color_eyre::eyre::Report;
use color_eyre::Result;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag};
use ratatui::widgets::Padding;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph},
    Frame,
};
use std::{
    io::Stdout,
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
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

fn highlight_markdown(content: &str) -> Vec<ratatui::text::Line<'static>> {
    use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag};
    let parser = Parser::new(content);
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();

    for event in parser {
        match event {
            MdEvent::Start(tag) => match tag {
                Tag::CodeBlock(info) => {
                    // Flush any pending normal text.
                    if !current_line.is_empty() {
                        lines.push(ratatui::text::Line::from(current_line.clone()));
                        current_line.clear();
                    }
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
                Tag::Heading(level, ..) => {
                    // For headings, prepend the appropriate number of '#' characters.
                    current_line.push_str(&"#".repeat(level as usize));
                    current_line.push(' ');
                }
                _ => {}
            },
            MdEvent::End(tag) => match tag {
                Tag::CodeBlock(_) => {
                    // Process the accumulated code block.
                    let syntax = SYNTAX_SET
                        .find_syntax_by_token(code_lang.as_str())
                        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
                    let theme = &THEME_SET.themes["base16-ocean.dark"];
                    let mut highlighter = HighlightLines::new(syntax, theme);
                    let code_lines: Vec<&str> = code_buffer.lines().collect();
                    for line in code_lines {
                        let ranges = highlighter.highlight(line, &SYNTAX_SET);
                        let spans: Vec<ratatui::text::Span> = ranges
                            .into_iter()
                            .map(|(s, text)| {
                                let fg = Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b);
                                ratatui::text::Span::styled(
                                    text.to_string(),
                                    Style::default().fg(fg),
                                )
                            })
                            .collect();
                        lines.push(ratatui::text::Line::from(spans));
                    }
                    code_buffer.clear();
                    in_code_block = false;
                }
                Tag::Heading(..) => {
                    // End of a heading: flush the current line.
                    lines.push(ratatui::text::Line::from(current_line.clone()));
                    current_line.clear();
                }
                _ => {}
            },
            MdEvent::Text(text) => {
                if in_code_block {
                    code_buffer.push_str(&text);
                } else {
                    current_line.push_str(&text);
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                if in_code_block {
                    code_buffer.push('\n');
                } else {
                    current_line.push('\n');
                    lines.push(ratatui::text::Line::from(current_line.clone()));
                    current_line.clear();
                }
            }
            _ => {}
        }
    }
    if !current_line.is_empty() {
        lines.push(ratatui::text::Line::from(current_line));
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

// Manually implement Debug for App, skipping search_interface.
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

    /// Setter for injecting a preconfigured SearchInterface.
    pub fn set_search_interface(&mut self, si: SearchInterface) {
        self.search_interface = Some(Arc::new(si));
    }

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
                            // Launch the config editor via our new module.
                            if let Err(e) = config_editor::open_config_in_editor(terminal) {
                                eprintln!("Error opening config: {}", e);
                            }
                            // Optionally, continue immediately after launching the editor.
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

    fn enter_search_mode(
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
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    ) {
        match key.code {
            KeyCode::Esc => {
                self.state = AppState::Preview;
            }
            KeyCode::Enter => {
                if let Some(doc) = self.search_results.get(self.selected_search_index) {
                    open_file_in_editor(terminal, &doc.path);
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

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        match self.state {
            AppState::Starting => {
                let paragraph = Paragraph::new("notemancy is starting")
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
                let paragraph = Paragraph::new(text)
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
                let paragraph = Paragraph::new(text)
                    .style(
                        Style::default()
                            .fg(Color::Rgb(224, 224, 224))
                            .bg(Color::Rgb(22, 22, 22)),
                    )
                    .block(Block::default());
                frame.render_widget(paragraph, area);
            }
            AppState::Preview => {
                let text = "Hello, Ratatui!\n\nCreated using https://github.com/ratatui/templates\nPress Ctrl+S to search.\nPress Esc, Ctrl-C or q to stop running.";
                let paragraph = Paragraph::new(text)
                    .style(
                        Style::default()
                            .fg(Color::Rgb(224, 224, 224))
                            .bg(Color::Rgb(22, 22, 22)),
                    )
                    .alignment(ratatui::layout::Alignment::Center)
                    .block(Block::default());
                frame.render_widget(paragraph, area);
            }
            AppState::Search => self.draw_search_ui(frame, area),
        }
    }

    fn draw_search_ui(&mut self, frame: &mut Frame, area: Rect) {
        // Use a smaller top chunk (2 rows instead of 3) for the input.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)].as_ref())
            .split(area);

        // Search input: apply internal padding by wrapping the text in spaces.
        let padded_input = format!(" {} ", self.search_query);
        let input = Line::from(padded_input).style(
            Style::default()
                .fg(Color::Rgb(69, 137, 255))
                .bg(Color::Rgb(30, 30, 30)),
        );
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
                    // Use blue background for the selected item.
                    Style::default()
                        .fg(Color::Rgb(224, 224, 224))
                        .bg(Color::Rgb(70, 130, 180)) // steel blue
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Rgb(198, 198, 198))
                        .bg(Color::Rgb(22, 22, 22))
                };
                ListItem::new(Span::styled(format!(" {} ", &doc.path), style))
            })
            .collect();

        let results_list = List::new(items).style(Style::default().bg(Color::Rgb(22, 22, 22)));
        frame.render_widget(results_list, bottom_chunks[0]);

        if let Some(doc) = self.search_results.get(self.selected_search_index) {
            let mut highlighted = highlight_full_markdown(&doc.content);
            // Overlay match highlighting if needed.
            if !self.search_query.is_empty() {
                highlighted = highlighted
                    .into_iter()
                    .map(|line| highlight_matches(&line, &self.search_query))
                    .collect();
            }
            // Insert extra spacing to simulate 1.5 line height.
            // let spaced_highlighted = add_line_spacing(highlighted);
            let preview_block = ratatui::widgets::Block::default().padding(Padding {
                left: (2),
                right: (2),
                top: (1),
                bottom: (1),
            });
            let preview = Paragraph::new(highlighted)
                .style(
                    Style::default()
                        .fg(Color::Rgb(224, 224, 224))
                        .bg(Color::Rgb(38, 38, 38)),
                )
                .alignment(ratatui::layout::Alignment::Left)
                .block(preview_block);
            frame.render_widget(preview, bottom_chunks[1]);
        } else {
            let preview = Paragraph::new("No file selected.")
                .style(
                    Style::default()
                        .fg(Color::Rgb(224, 224, 224))
                        .bg(Color::Rgb(38, 38, 38)),
                )
                .alignment(ratatui::layout::Alignment::Left);
            frame.render_widget(preview, bottom_chunks[1]);
        }
    }

    fn quit(&mut self) {
        self.running = false;
    }
}

/// Opens the specified file in the default editor (using $EDITOR or "vi").
/// Restores the terminal, launches the editor, waits for it to exit, then reinitializes the terminal.
fn open_file_in_editor(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    path: &str,
) {
    ratatui::restore();
    crossterm::terminal::disable_raw_mode().expect("Failed to disable raw mode");
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let _ = std::process::Command::new(editor).arg(path).status();
    *terminal = ratatui::init();
}

fn highlight_matches(line: &ratatui::text::Line, query: &str) -> ratatui::text::Line<'static> {
    let mut new_spans = Vec::new();
    // Iterate over the spans in the line.
    for span in &line.spans {
        // Convert the span content to an owned String.
        let text = span.content.to_string();
        let mut start = 0;
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        while let Some(pos) = text_lower[start..].find(&query_lower) {
            let pos = start + pos;
            if pos > start {
                new_spans.push(ratatui::text::Span::styled(
                    text[start..pos].to_string(),
                    span.style,
                ));
            }
            new_spans.push(ratatui::text::Span::styled(
                text[pos..pos + query.len()].to_string(),
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(ratatui::style::Color::Yellow),
            ));
            start = pos + query.len();
        }
        if start < text.len() {
            new_spans.push(ratatui::text::Span::styled(
                text[start..].to_string(),
                span.style,
            ));
        }
    }
    ratatui::text::Line::from(new_spans)
}

fn highlight_full_markdown(content: &str) -> Vec<Line<'static>> {
    let parser = Parser::new(content);
    let mut lines = Vec::new();
    let mut current_spans = Vec::new();

    // For code block processing
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();

    for event in parser {
        match event {
            MdEvent::Start(tag) => match tag {
                Tag::CodeBlock(info) => {
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
                Tag::Heading(level, ..) => {
                    // Prepend heading markers styled in blue and bold.
                    let markers = format!("{} ", "#".repeat(level as usize));
                    current_spans.push(Span::styled(
                        markers,
                        Style::default()
                            .fg(Color::Rgb(69, 137, 255))
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                Tag::List(_) => {
                    // Prepend a bullet for list items.
                    current_spans.push(Span::styled(
                        "• ",
                        Style::default().fg(Color::Rgb(69, 137, 255)),
                    ));
                }
                // You can add additional styling for Emphasis, Strong, etc. here.
                _ => {}
            },
            MdEvent::End(tag) => match tag {
                Tag::CodeBlock(_) => {
                    // Process code block using your existing syntect approach.
                    use once_cell::sync::Lazy;
                    use syntect::easy::HighlightLines;
                    use syntect::highlighting::ThemeSet;
                    use syntect::parsing::SyntaxSet;
                    static SYNTAX_SET: Lazy<SyntaxSet> =
                        Lazy::new(|| SyntaxSet::load_defaults_newlines());
                    static THEME_SET: Lazy<ThemeSet> = Lazy::new(|| ThemeSet::load_defaults());
                    let syntax = SYNTAX_SET
                        .find_syntax_by_token(code_lang.as_str())
                        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
                    let theme = &THEME_SET.themes["base16-ocean.dark"];
                    let mut highlighter = HighlightLines::new(syntax, theme);
                    // Process each line in the code block.
                    for line in code_buffer.lines() {
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
                Tag::Heading(..) | Tag::List(_) | Tag::Paragraph => {
                    // End of a block: flush current spans as a new line.
                    if !current_spans.is_empty() {
                        lines.push(Line::from(current_spans));
                        current_spans = Vec::new();
                    }
                }
                _ => {}
            },
            MdEvent::Text(text) => {
                if in_code_block {
                    code_buffer.push_str(&text);
                } else {
                    current_spans.push(Span::raw(text.to_string()));
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                if in_code_block {
                    code_buffer.push('\n');
                } else {
                    // End the current line.
                    lines.push(Line::from(current_spans));
                    current_spans = Vec::new();
                }
            }
            _ => {}
        }
    }
    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }
    lines
}

fn add_line_spacing(lines: Vec<ratatui::text::Line<'static>>) -> Vec<ratatui::text::Line<'static>> {
    let mut spaced = Vec::new();
    for line in lines {
        spaced.push(line);
        // Insert an empty line (or a line with a single space) after every line.
        spaced.push(ratatui::text::Line::from(" "));
    }
    spaced
}
