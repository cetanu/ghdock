use chrono::{DateTime, Local, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};

use crate::{app::App, domain::Pull};

pub(crate) fn draw(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(7),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, chunks[0], app);
    draw_table(frame, chunks[1], app);
    draw_footer(frame, chunks[2], app);
    if app.is_help_visible() {
        draw_help(frame, area);
    }
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let status = if let Some(error) = app.error() {
        format!(
            " ERROR  {}",
            truncate(error, area.width.saturating_sub(12) as usize)
        )
    } else if app.is_loading() {
        " SYNCING  GitHub inbox…".to_string()
    } else {
        format!(
            " LIVE  next poll in {}s",
            app.next_poll()
                .saturating_duration_since(std::time::Instant::now())
                .as_secs()
        )
    };
    let title = Line::from(vec![
        Span::styled(
            " gh",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "dock",
            Style::default()
                .fg(Color::Rgb(255, 159, 67))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  /  pull request inbox", Style::default().fg(Color::Gray)),
    ]);
    let sync_age = app
        .last_fetch()
        .map(|fetched| format!("   synced {}s ago", fetched.elapsed().as_secs()))
        .unwrap_or_default();
    let status_line = Line::from(vec![
        Span::styled(
            status,
            Style::default().fg(if app.error().is_some() {
                Color::Red
            } else {
                Color::Green
            }),
        ),
        Span::styled(
            format!("   {} pull requests", app.pulls().len()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(sync_age, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(
        Paragraph::new(Text::from(vec![title, status_line])).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::Rgb(58, 64, 75))),
        ),
        area,
    );
}

fn draw_table(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "",
        "REPOSITORY",
        "PULL REQUEST",
        "ACTIVITY",
        "STATE",
        "UPDATED",
    ])
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .height(1);
    let entries = grouped_entries(app);
    let rows = entries.iter().map(|entry| match entry {
        InboxEntry::Group(label) => Row::new(vec![
            Cell::from("").style(Style::default().fg(Color::DarkGray)),
            Cell::from(*label).style(
                Style::default()
                    .fg(Color::Rgb(255, 159, 67))
                    .add_modifier(Modifier::BOLD),
            ),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ])
        .height(1),
        InboxEntry::Pull(index) => pull_row(&app.pulls()[*index]),
    });
    let widths = [
        Constraint::Length(2),
        Constraint::Length(25),
        Constraint::Min(28),
        Constraint::Length(20),
        Constraint::Length(9),
        Constraint::Length(12),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(35, 42, 56))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ")
        .block(
            Block::default()
                .title(Span::styled(
                    "  INBOX  ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(58, 64, 75))),
        );
    let mut table_state = TableState::default();
    table_state.select(entries.iter().position(
        |entry| matches!(entry, InboxEntry::Pull(index) if *index == app.selected_index()),
    ));
    frame.render_stateful_widget(table, area, &mut table_state);

    if let Some(pull) = app.selected_pull() {
        draw_detail(frame, area, pull);
    }
}

enum InboxEntry {
    Group(&'static str),
    Pull(usize),
}

fn grouped_entries(app: &App) -> Vec<InboxEntry> {
    ["OPEN", "DRAFT", "CLOSED", "UNKNOWN"]
        .into_iter()
        .flat_map(|label| {
            let indices = app
                .pulls()
                .iter()
                .enumerate()
                .filter_map(|(index, pull)| (pull.status_label() == label).then_some(index))
                .collect::<Vec<_>>();
            if indices.is_empty() {
                Vec::new()
            } else {
                std::iter::once(InboxEntry::Group(label))
                    .chain(indices.into_iter().map(InboxEntry::Pull))
                    .collect()
            }
        })
        .collect()
}

fn pull_row(pull: &Pull) -> Row<'static> {
    let unread = if pull.unread { "●" } else { " " };
    let state_color = match pull.state.as_str() {
        "open" => Color::Green,
        "closed" => Color::Red,
        _ => Color::Yellow,
    };
    Row::new(vec![
        Cell::from(unread).style(Style::default().fg(if pull.unread {
            Color::Rgb(255, 159, 67)
        } else {
            Color::DarkGray
        })),
        Cell::from(pull.repo.clone()).style(Style::default().fg(Color::Gray)),
        Cell::from(format!("#{} {}", pull.number, truncate(&pull.title, 54))),
        Cell::from(pull.activity_label()).style(Style::default().fg(Color::Rgb(164, 174, 196))),
        Cell::from(pull.status_label()).style(
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(format_updated(&pull.updated_at)).style(Style::default().fg(Color::DarkGray)),
    ])
    .height(1)
}

fn draw_detail(frame: &mut ratatui::Frame, area: Rect, pull: &Pull) {
    if area.height < 10 {
        return;
    }
    let detail_height = 4;
    let detail_area = Rect {
        x: area.x + 1,
        y: area.bottom().saturating_sub(detail_height + 1),
        width: area.width.saturating_sub(2),
        height: detail_height,
    };
    let stats = format!(
        "{}  ·  {}  ·  {} commits  ·  +{} -{}  ·  {} files",
        pull.author,
        pull.activity_label(),
        pull.commits,
        pull.additions,
        pull.deletions,
        pull.changed_files
    );
    let content = vec![
        Line::from(Span::styled(
            truncate(&pull.title, detail_area.width.saturating_sub(4) as usize),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(stats, Style::default().fg(Color::Gray))),
    ];
    frame.render_widget(
        Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::Rgb(58, 64, 75))),
            )
            .wrap(Wrap { trim: true }),
        detail_area,
    );
}

fn draw_footer(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let left = " ↑↓/jk navigate   enter/o open   r refresh   ? help   q quit";
    let line = if let Some(alert) = app.alert() {
        Line::from(vec![
            Span::styled(
                " ALERT  ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(255, 159, 67))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " {}",
                    truncate(alert, area.width.saturating_sub(10) as usize)
                ),
                Style::default().fg(Color::Rgb(255, 197, 122)),
            ),
        ])
    } else {
        Line::from(Span::styled(left, Style::default().fg(Color::DarkGray)))
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_help(frame: &mut ratatui::Frame, area: Rect) {
    let popup = centered_rect(64, 48, area);
    frame.render_widget(Clear, popup);
    let text = vec![
        Line::from(Span::styled(
            "ghdock controls",
            Style::default()
                .fg(Color::Rgb(255, 159, 67))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("↑ / k       select previous pull request"),
        Line::from("↓ / j       select next pull request"),
        Line::from("enter / o    open the selected pull request"),
        Line::from("r            refresh after the current poll"),
        Line::from("q / esc      quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Alerts fire after the first sync when a pull request is new or its state, comments, title, or activity changes.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "press any of ? / esc / enter to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" HELP ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(255, 159, 67))),
            )
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn format_updated(value: &str) -> String {
    let Ok(date) = DateTime::parse_from_rfc3339(value) else {
        return value.to_string();
    };
    let local = date.with_timezone(&Local);
    let age = Utc::now().signed_duration_since(local.with_timezone(&Utc));
    if age.num_minutes() < 1 {
        "just now".into()
    } else if age.num_hours() < 1 {
        format!("{}m ago", age.num_minutes())
    } else if age.num_days() < 1 {
        format!("{}h ago", age.num_hours())
    } else if age.num_days() < 7 {
        format!("{}d ago", age.num_days())
    } else {
        local.format("%d %b").to_string()
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else if max < 2 {
        "…".to_string()
    } else {
        format!("{}…", value.chars().take(max - 1).collect::<String>())
    }
}
