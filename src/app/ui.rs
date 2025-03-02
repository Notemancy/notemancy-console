use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    widgets::{List, ListItem, Padding},
    Frame,
};

use crate::app::core::App;
use crate::app::highlight::{highlight_full_markdown, highlight_matches};
use std::fs;
use std::path::Path;

pub fn draw_search_ui(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)].as_ref())
        .split(area);

    let padded_input = format!(" {} ", app.search_query);
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

    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let style = if i == app.selected_search_index {
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

            // Display title (if available) or just the filename
            let display_text = if result.title.is_empty() {
                // Extract filename from path if no title
                let path = std::path::Path::new(&result.path);
                path.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&result.path)
                    .to_string()
            } else {
                result.title.clone()
            };

            ListItem::new(Span::styled(format!(" {} ", display_text), style))
        })
        .collect();

    let results_list = List::new(items).style(Style::default().bg(Color::Rgb(22, 22, 22)));
    frame.render_widget(results_list, bottom_chunks[0]);

    if let Some(result) = app.search_results.get(app.selected_search_index) {
        // Read the content of the selected file
        let content = match fs::read_to_string(&result.path) {
            Ok(content) => content,
            Err(e) => {
                format!("Error reading file: {}", e)
            }
        };

        let mut highlighted = highlight_full_markdown(&content);
        // Overlay match highlighting if needed.
        if !app.search_query.is_empty() {
            highlighted = highlighted
                .into_iter()
                .map(|line| highlight_matches(&line, &app.search_query))
                .collect();
        }

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

pub fn draw_command_palette(app: &App, frame: &mut Frame, area: Rect) {
    use ratatui::widgets::{Block, Borders, List, ListItem};
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Command Palette")
        .border_style(Style::default().fg(Color::Cyan));

    let inner_area = centered_rect(60, 30, area);
    let items: Vec<ListItem> = app
        .command_items
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let style = if i == app.selected_command_index {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let content = format!("{} - {}", cmd.name, cmd.description);
            ListItem::new(Span::styled(content, style))
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, inner_area);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);
    let vertical_chunk = popup_layout[1];
    let horizontal_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(vertical_chunk);
    horizontal_layout[1]
}

pub fn draw_find_related_ui(app: &mut App, frame: &mut Frame) {
    let area = frame.area();

    // Top: search input area.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)].as_ref())
        .split(area);

    let padded_input = format!(" {} ", app.search_query);
    let input = Paragraph::new(Line::from(padded_input)).style(
        Style::default()
            .fg(Color::Rgb(69, 137, 255))
            .bg(Color::Rgb(30, 30, 30)),
    );
    frame.render_widget(input, chunks[0]);

    // Lower area: split horizontally into two equal halves.
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    // Left side: render search results.
    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let style = if i == app.selected_search_index {
                Style::default()
                    .fg(Color::Rgb(224, 224, 224))
                    .bg(Color::Rgb(70, 130, 180)) // steel blue
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Rgb(198, 198, 198))
                    .bg(Color::Rgb(22, 22, 22))
            };

            // Use the file name or title.
            let display_text = if result.title.is_empty() {
                let path = Path::new(&result.path);
                path.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&result.path)
                    .to_string()
            } else {
                result.title.clone()
            };

            ListItem::new(Span::styled(format!(" {} ", display_text), style))
        })
        .collect();

    let results_list = List::new(items).style(Style::default().bg(Color::Rgb(22, 22, 22)));
    frame.render_widget(results_list, bottom_chunks[0]);

    // Right side: simply display a placeholder (or leave blank).
    let placeholder = Paragraph::new(" ").style(Style::default().bg(Color::Rgb(22, 22, 22)));
    frame.render_widget(placeholder, bottom_chunks[1]);
}

pub fn draw_vector_indexing_ui(app: &App, frame: &mut Frame, area: Rect) {
    // Create a popup in the center of the screen
    let popup_area = centered_rect(50, 30, area);

    // Create a block with a border for the popup
    let block = Block::default()
        .title("Vector Indexing")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Render the block in the frame
    frame.render_widget(block, popup_area);

    // Create an inner area for the content
    let inner_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 2,
        width: popup_area.width - 4,
        height: popup_area.height - 4,
    };

    // Get the status message from the app
    let status_message = app
        .vector_indexing_status
        .as_deref()
        .unwrap_or("Initializing...");

    // Create a spinner character based on the app's spinner index
    let spinner = app.spinner_chars[app.spinner_idx];

    // Create the text to display
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "Generating vector embeddings for all markdown files",
            Style::default().fg(Color::White),
        )]),
        Line::from(""), // Empty line for spacing
    ];

    // If indexing is complete, show a success message
    if app.vector_indexing_complete {
        lines.push(Line::from(vec![
            Span::styled(
                "✓ ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(status_message, Style::default().fg(Color::Green)),
        ]));
    } else {
        // Otherwise show the spinner and current status
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", spinner), Style::default().fg(Color::Yellow)),
            Span::styled(status_message, Style::default().fg(Color::White)),
        ]));
    }

    // Add an extra line with instructions
    if app.vector_indexing_complete {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Returning to main menu...",
            Style::default().fg(Color::Gray),
        )]));
    }

    // Create a paragraph with all the lines
    let paragraph = Paragraph::new(lines)
        .style(Style::default().bg(Color::Rgb(22, 22, 22)))
        .alignment(ratatui::layout::Alignment::Center);

    // Render the paragraph in the inner area
    frame.render_widget(paragraph, inner_area);
}
