pub mod assets;
pub mod download;
pub mod footer;
pub mod header;
pub mod help;
pub mod main_view;
pub mod presets;

use crate::app::{App, CurrentView};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn ui(f: &mut Frame, app: &mut App) {
    let size = f.area();

    if size.width < 60 || size.height < 16 {
        let p = Paragraph::new("Terminal Too Small")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(p, size);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3), // Header
                Constraint::Min(0),    // Main Content
                Constraint::Length(3), // Footer
            ]
            .as_ref(),
        )
        .split(size);

    header::render_header(f, app, chunks[0]);

    match app.view {
        CurrentView::Main => main_view::render_grid(f, app, chunks[1]),
        CurrentView::Presets => presets::render_presets(f, app, chunks[1]),
        CurrentView::Downloads => download::render_downloads_view(f, app, chunks[1]),
        CurrentView::Help => {
            main_view::render_grid(f, app, chunks[1]);
            help::render_help(f, size);
        }
        CurrentView::AssetMissing => assets::render_asset_prompt(f, app, chunks[1]),
        CurrentView::DownloadingAssets => assets::render_asset_download(f, app, chunks[1]),
    }

    footer::render_footer(f, app, chunks[2]);
}
