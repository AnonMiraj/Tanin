use crate::app::{App, CurrentView};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

pub fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Min(0),
            Constraint::Length(20),
        ])
        .split(area);

    // Left: Title
    let title = Span::styled(
        "♫ tanin ",
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    );
    let p_title = Paragraph::new(title).alignment(Alignment::Left);
    f.render_widget(p_title, chunks[0]);

    // Center: Tabs or Search
    if app.search_mode || !app.search_query.is_empty() {
        let search_text = format!("Search: {}_", app.search_query);
        let style = if app.search_mode {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Yellow)
        };
        let p = Paragraph::new(search_text)
            .style(style)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(p, chunks[1]);
    } else {
        let mut titles = vec![" Sounds ", " Presets "];
        if app.yt_dlp_available {
            titles.push(" Downloads ");
        }

        let selected_tab = match app.view {
            CurrentView::Main | CurrentView::Help => 0,
            CurrentView::Presets => 1,
            CurrentView::Downloads => 2,
            CurrentView::AssetMissing | CurrentView::DownloadingAssets => 0,
        };

        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::NONE))
            .select(selected_tab)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .divider(Span::raw("|"));

        f.render_widget(tabs, chunks[1]);
    }

    // Right: Status
    let active_count = if let Some(engine) = &app.audio_engine {
        app.sounds
            .iter()
            .filter(|s| engine.is_playing(&s.id))
            .count()
    } else {
        0
    };

    let mut right_spans = vec![];

    if active_count > 0 {
        right_spans.push(Span::styled(
            format!(" ▶ {} ", active_count),
            Style::default().bg(Color::Green).fg(Color::White),
        ));
        right_spans.push(Span::raw("  "));
    }
    right_spans.push(Span::styled("? help", Style::default().fg(Color::DarkGray)));

    let p_right = Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right);
    f.render_widget(p_right, chunks[2]);
}
