use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag};
use ratatui::style::Modifier;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
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

pub fn highlight_full_markdown(content: &str) -> Vec<Line<'static>> {
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
                        Lazy::new(SyntaxSet::load_defaults_newlines);
                    static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);
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

pub fn highlight_matches(line: &Line, query: &str) -> Line<'static> {
    let mut new_spans = Vec::new();
    for span in &line.spans {
        let text = span.content.to_string();
        let mut start = 0;
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        while let Some(pos) = text_lower[start..].find(&query_lower) {
            let pos = start + pos;
            if pos > start {
                new_spans.push(Span::styled(text[start..pos].to_string(), span.style));
            }
            new_spans.push(Span::styled(
                text[pos..pos + query.len()].to_string(),
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ));
            start = pos + query.len();
        }
        if start < text.len() {
            new_spans.push(Span::styled(text[start..].to_string(), span.style));
        }
    }
    Line::from(new_spans)
}

pub fn add_line_spacing(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let mut spaced = Vec::new();
    for line in lines {
        spaced.push(line);
        spaced.push(Line::from(" "));
    }
    spaced
}
