use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Padding, Paragraph},
    Frame,
};

use crate::app::core::App;
use crate::app::highlight::{highlight_full_markdown, highlight_matches};

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
        .map(|(i, doc)| {
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
            ListItem::new(Span::styled(format!(" {} ", &doc.path), style))
        })
        .collect();

    let results_list = List::new(items).style(Style::default().bg(Color::Rgb(22, 22, 22)));
    frame.render_widget(results_list, bottom_chunks[0]);

    if let Some(doc) = app.search_results.get(app.selected_search_index) {
        let mut highlighted = highlight_full_markdown(&doc.content);
        // Overlay match highlighting if needed.
        if !app.search_query.is_empty() {
            highlighted = highlighted
                .into_iter()
                .map(|line| highlight_matches(&line, &app.search_query))
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
