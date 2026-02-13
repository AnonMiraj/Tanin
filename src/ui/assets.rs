use crate::app::App;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub fn render_asset_prompt(f: &mut Frame, _app: &App, area: Rect) {
    let block = Block::default()
        .title(" Missing Assets ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let text = vec![
        "Bundled sound assets are missing.",
        "They are required for the default experience.",
        "",
        "Download them from GitHub? (~17MB)",
        "",
        "[Enter] Download    [Esc] Skip (Empty app)",
    ];

    let p = Paragraph::new(text.join("\n"))
        .block(block)
        .alignment(Alignment::Center);

    let area = center_rect(area, 60, 10);
    f.render_widget(Clear, area);
    f.render_widget(p, area);
}

pub fn render_asset_download(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Downloading Assets ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let area = center_rect(area, 60, 10);
    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ]
            .as_ref(),
        )
        .split(area);

    if let Some(err) = &app.asset_download_error {
        let p = Paragraph::new(format!("Error: {}", err))
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        f.render_widget(p, chunks[0]);

        let sub = Paragraph::new("[Esc] Continue without assets").alignment(Alignment::Center);
        f.render_widget(sub, chunks[2]);
    } else {
        let p = Paragraph::new("Downloading configuration...").alignment(Alignment::Center);
        f.render_widget(p, chunks[1]);
    }
}

fn center_rect(r: Rect, w: u16, h: u16) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length((r.height.saturating_sub(h)) / 2),
                Constraint::Length(h),
                Constraint::Min(0),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Length((r.width.saturating_sub(w)) / 2),
                Constraint::Length(w),
                Constraint::Min(0),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
