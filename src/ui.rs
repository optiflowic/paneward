//! Rendering with ratatui. Reads the app state only; never runs commands or touches files.

use clap::ValueEnum;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};

use crate::app::{App, ListedSession, MAX_JUMP_NUMBER};
use crate::state::Status;

/// `auto` uses the dock when the area is wider than this many times its height.
/// Terminal cells are about twice as tall as wide, so 2 means "wider than square".
const DOCK_ASPECT_RATIO: u32 = 2;

/// Messages wrap over at most this many lines, so the list keeps most of the room.
const MAX_MESSAGE_LINES: u16 = 3;

const TITLE: &str = " paneward ";
const SIDEBAR_HINTS: &str = " ⏎ jump  q quit ";
const DOCK_HINTS: &str = " ↑↓ move  ⏎ jump  q quit ";
const HIGHLIGHT_SYMBOL: &str = "▶ ";

const DOCK_COLUMN_SPACING: u16 = 2;
/// Width of the jump number and status icon, `1 ●`.
const MARKER_WIDTH: u16 = 3;
/// Width of the longest status label (`waiting`, `running`).
const STATUS_WIDTH: u16 = 7;
/// Width of the longest elapsed time below 100 days (`99d`), plus room for `100d`.
const ELAPSED_WIDTH: u16 = 4;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;

/// The layout asked for on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LayoutChoice {
    /// Sidebar in tall panes, dock in wide ones.
    Auto,
    /// Two lines per session, for a narrow, tall split.
    Sidebar,
    /// One line per session, for a wide, short split.
    Dock,
}

#[derive(Debug, PartialEq, Eq)]
enum Shape {
    Sidebar,
    Dock,
}

/// Draws the whole screen. `now` is Unix seconds, for elapsed times.
pub fn draw(frame: &mut Frame, app: &App, layout: LayoutChoice, now: u64) {
    let area = frame.area();
    let shape = shape_for(layout, area);
    let hints = match shape {
        Shape::Sidebar => SIDEBAR_HINTS,
        Shape::Dock => DOCK_HINTS,
    };
    let block = Block::bordered()
        .title(TITLE.bold())
        .title_bottom(Line::from(hints).dim());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (list_area, message_area) = split_for_message(inner, app.message());
    if let (Some(message), Some(message_area)) = (app.message(), message_area) {
        let text = Paragraph::new(message)
            .style(Style::new().fg(Color::Yellow))
            .wrap(Wrap { trim: true });
        frame.render_widget(text, message_area);
    }

    if app.sessions().is_empty() {
        frame.render_widget(Paragraph::new("No sessions").dim(), list_area);
        return;
    }
    match shape {
        Shape::Sidebar => draw_sidebar(frame, app, list_area, now),
        Shape::Dock => draw_dock(frame, app, list_area, now),
    }
}

fn shape_for(layout: LayoutChoice, area: Rect) -> Shape {
    match layout {
        LayoutChoice::Sidebar => Shape::Sidebar,
        LayoutChoice::Dock => Shape::Dock,
        LayoutChoice::Auto => auto_shape(area),
    }
}

fn auto_shape(area: Rect) -> Shape {
    if u32::from(area.width) > u32::from(area.height) * DOCK_ASPECT_RATIO {
        Shape::Dock
    } else {
        Shape::Sidebar
    }
}

/// Splits off the bottom lines for the message, if there is one.
fn split_for_message(area: Rect, message: Option<&str>) -> (Rect, Option<Rect>) {
    let Some(message) = message else {
        return (area, None);
    };
    let height = message_height(message, area.width);
    let [list, message] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(height)]).areas(area);
    (list, Some(message))
}

fn message_height(message: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let lines: usize = message
        .lines()
        .map(|line| Line::from(line).width().div_ceil(width).max(1))
        .sum();
    u16::try_from(lines)
        .unwrap_or(MAX_MESSAGE_LINES)
        .clamp(1, MAX_MESSAGE_LINES)
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect, now: u64) {
    let items: Vec<ListItem> = app
        .sessions()
        .iter()
        .enumerate()
        .map(|(index, listed)| sidebar_item(index, listed, now))
        .collect();
    let list = List::new(items)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_style(Modifier::BOLD);
    let mut state = ListState::default().with_selected(Some(app.selected()));
    frame.render_stateful_widget(list, area, &mut state);
}

/// `1 ● project` over `    agent  status  2m`.
fn sidebar_item(index: usize, listed: &ListedSession, now: u64) -> ListItem<'static> {
    let session = &listed.session;
    let title = Line::from(vec![
        jump_number(index),
        Span::raw(" "),
        status_icon(session.status),
        Span::raw(" "),
        Span::raw(session.project.clone()).bold(),
    ]);
    let details = Line::from(format!(
        "    {}  {}  {}",
        session.agent,
        status_label(session.status),
        format_elapsed(now.saturating_sub(listed.status_since)),
    ))
    .dim();
    ListItem::new(vec![title, details])
}

fn draw_dock(frame: &mut Frame, app: &App, area: Rect, now: u64) {
    let rows = app
        .sessions()
        .iter()
        .enumerate()
        .map(|(index, listed)| dock_row(index, listed, now));
    let sessions = || app.sessions().iter().map(|listed| &listed.session);
    // Columns as wide as their content, packed to the left.
    let widths = [
        Constraint::Length(MARKER_WIDTH),
        Constraint::Length(STATUS_WIDTH),
        Constraint::Length(widest(sessions().map(|session| session.project.as_str()))),
        Constraint::Length(widest(sessions().map(|session| session.agent.as_str()))),
        Constraint::Length(ELAPSED_WIDTH),
    ];
    let table = Table::new(rows, widths)
        .column_spacing(DOCK_COLUMN_SPACING)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .row_highlight_style(Modifier::BOLD);
    let mut state = TableState::default().with_selected(Some(app.selected()));
    frame.render_stateful_widget(table, area, &mut state);
}

/// Display width of the widest text, saturating at `u16::MAX`.
fn widest<'a>(texts: impl Iterator<Item = &'a str>) -> u16 {
    let width = texts.map(|text| Span::raw(text).width()).max().unwrap_or(0);
    u16::try_from(width).unwrap_or(u16::MAX)
}

/// `1 ● waiting  project  agent  2m`.
fn dock_row(index: usize, listed: &ListedSession, now: u64) -> Row<'static> {
    let session = &listed.session;
    Row::new(vec![
        Line::from(vec![
            jump_number(index),
            Span::raw(" "),
            status_icon(session.status),
        ]),
        Line::from(status_label(session.status)),
        Line::from(session.project.clone()).bold(),
        Line::from(session.agent.clone()).dim(),
        Line::from(format_elapsed(now.saturating_sub(listed.status_since))).dim(),
    ])
}

/// The number key that jumps to this row, blank past the last number key.
fn jump_number(index: usize) -> Span<'static> {
    let number = index + 1;
    let text = if number <= MAX_JUMP_NUMBER {
        number.to_string()
    } else {
        " ".to_string()
    };
    Span::raw(text).dim()
}

fn status_icon(status: Status) -> Span<'static> {
    match status {
        Status::Waiting => Span::styled("●", Style::new().fg(Color::Yellow)),
        Status::Running => Span::styled("◐", Style::new().fg(Color::Green)),
        Status::Idle => Span::styled("○", Style::new().fg(Color::DarkGray)),
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Waiting => "waiting",
        Status::Running => "running",
        Status::Idle => "idle",
    }
}

/// Formats a duration in its largest whole unit: `12s`, `5m`, `3h`, `2d`.
fn format_elapsed(seconds: u64) -> String {
    match seconds {
        s if s < SECONDS_PER_MINUTE => format!("{s}s"),
        s if s < SECONDS_PER_HOUR => format!("{}m", s / SECONDS_PER_MINUTE),
        s if s < SECONDS_PER_DAY => format!("{}h", s / SECONDS_PER_HOUR),
        s => format!("{}d", s / SECONDS_PER_DAY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_elapsed_time_in_the_largest_whole_unit() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(59), "59s");
        assert_eq!(format_elapsed(60), "1m");
        assert_eq!(format_elapsed(3_599), "59m");
        assert_eq!(format_elapsed(3_600), "1h");
        assert_eq!(format_elapsed(86_399), "23h");
        assert_eq!(format_elapsed(86_400), "1d");
        assert_eq!(format_elapsed(10 * 86_400), "10d");
    }

    #[test]
    fn auto_layout_uses_the_dock_only_in_wide_areas() {
        let area = |width, height| Rect::new(0, 0, width, height);
        assert_eq!(shape_for(LayoutChoice::Auto, area(30, 60)), Shape::Sidebar);
        assert_eq!(shape_for(LayoutChoice::Auto, area(100, 50)), Shape::Sidebar);
        assert_eq!(shape_for(LayoutChoice::Auto, area(101, 50)), Shape::Dock);
        assert_eq!(shape_for(LayoutChoice::Auto, area(200, 8)), Shape::Dock);
    }

    #[test]
    fn explicit_layouts_ignore_the_area() {
        let wide = Rect::new(0, 0, 200, 8);
        let tall = Rect::new(0, 0, 30, 60);
        assert_eq!(shape_for(LayoutChoice::Sidebar, wide), Shape::Sidebar);
        assert_eq!(shape_for(LayoutChoice::Dock, tall), Shape::Dock);
    }

    #[test]
    fn message_height_wraps_up_to_a_limit() {
        assert_eq!(message_height("short", 28), 1);
        assert_eq!(message_height(&"x".repeat(29), 28), 2);
        assert_eq!(message_height(&"x".repeat(500), 28), MAX_MESSAGE_LINES);
        assert_eq!(message_height("x", 0), 1);
    }

    #[test]
    fn message_height_counts_each_line_of_a_multi_line_message() {
        assert_eq!(message_height("first\nsecond", 28), 2);
        assert_eq!(message_height(&format!("{}\nshort", "x".repeat(29)), 28), 3);
    }

    #[test]
    fn widest_measures_display_width() {
        assert_eq!(widest(["kumolo", "paneward", "a"].into_iter()), 8);
        assert_eq!(widest(["日本語"].into_iter()), 6);
        assert_eq!(widest(std::iter::empty()), 0);
    }

    #[test]
    fn rows_past_nine_have_no_jump_number() {
        assert_eq!(jump_number(0).content, "1");
        assert_eq!(jump_number(8).content, "9");
        assert_eq!(jump_number(9).content, " ");
    }
}
