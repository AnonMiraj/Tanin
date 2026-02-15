use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub fn render_help(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled(
            "⌨  Keyboard Shortcuts",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Navigation",
            Style::default().fg(Color::Green),
        )),
        Line::from("  Tab             Switch Views"),
        Line::from("  /               Search Sounds"),
        Line::from("  h j k l         Navigate Grid / List"),
        Line::from("  a               Download Custom Sounds (require yt-dlp) "),
        Line::from(""),
        Line::from(Span::styled(
            "Sounds View",
            Style::default().fg(Color::Green),
        )),
        Line::from("  Enter / Space   Toggle sound"),
        Line::from("  < / >           Master Volume"),
        Line::from("  + / -           Volume"),
        Line::from("  s               Stop all"),
        Line::from(""),
        Line::from(Span::styled(
            "Presets View",
            Style::default().fg(Color::Green),
        )),
        Line::from("  n               Create New Preset"),
        Line::from("  r               Rename Selected Preset"),
        Line::from("  u               Update Preset (Overwrite with current)"),
        Line::from("  d               Delete Preset"),
        Line::from("  Enter           Load Preset"),
        Line::from(""),
        Line::from(Span::styled("General", Style::default().fg(Color::Green))),
        Line::from("  m               Mute Master"),
        Line::from("  ?               Toggle Help"),
        Line::from("  q               Quit"),
    ];

    let width = 60;
    let height = help_text.len() as u16 + 2;

    let area = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Help")
        .style(Style::default().bg(Color::Black));
    let p = Paragraph::new(help_text)
        .block(block)
        .alignment(Alignment::Left);

    f.render_widget(p, area);
}
