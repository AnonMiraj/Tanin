use crate::app::App;
use crate::static_data::Sound;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

pub fn render_grid(f: &mut Frame, app: &App, area: Rect) {
    let mut current_y: i32 = area.y as i32 - app.grid_scroll as i32;
    let card_width = 24;
    let card_height = 5;
    let col_width = card_width + 2;
    let cols = app.grid_cols;

    let filtered = app.get_filtered_sounds();

    if filtered.is_empty() {
        let msg = if app.search_query.is_empty() {
            "No sounds available.\nAdd custom sounds or check assets."
        } else {
            "No sounds match your search."
        };

        let p = Paragraph::new(msg)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::NONE));

        // Center vertically in the area
        let center_y = area.height / 2;
        let msg_area = Rect::new(area.x, area.y + center_y.saturating_sub(1), area.width, 2);

        f.render_widget(p, msg_area);
        return;
    }

    let mut categories: Vec<String> = filtered.iter().map(|(_, s)| s.category.clone()).collect();
    categories.dedup();

    for category in categories {
        if current_y > area.bottom() as i32 {
            break;
        }

        // Render header if visible
        if current_y + 1 >= area.top() as i32 && current_y < area.bottom() as i32 {
            let header_rect = Rect::new(area.x + 2, current_y as u16, area.width - 4, 1);
            f.render_widget(
                Paragraph::new(format!("─── {} ───", category))
                    .style(Style::default().fg(Color::DarkGray)),
                header_rect,
            );
        }
        current_y += 2;

        let sounds_in_cat: Vec<(usize, &Sound)> = filtered
            .iter()
            .filter(|(_, s)| s.category == category)
            .map(|(i, s)| (*i, *s))
            .collect();

        for chunk in sounds_in_cat.chunks(cols as usize) {
            if current_y > area.bottom() as i32 {
                break;
            }

            if current_y + card_height as i32 > area.top() as i32 {
                for (i, (global_idx, sound)) in chunk.iter().enumerate() {
                    let x = area.x + 2 + (i as u16 * col_width);
                    if x + card_width > area.right() {
                        continue;
                    }

                    let card_y = current_y;
                    if card_y < 0 {
                        continue;
                    }

                    let visible_height =
                        std::cmp::min(card_height, (area.bottom() as i32 - card_y).max(0) as u16);

                    if visible_height > 0 && card_y >= area.top() as i32 {
                        let rect = Rect::new(x, card_y as u16, card_width, visible_height);
                        render_card(f, app, *global_idx, sound, rect);
                    }
                }
            }
            current_y += card_height as i32 + 1;
        }
    }
}

fn render_card(f: &mut Frame, app: &App, idx: usize, sound: &Sound, area: Rect) {
    let selected = idx == app.cursor_pos;
    let playing = if let Some(engine) = &app.audio_engine {
        engine.is_playing(&sound.id)
    } else {
        false
    };

    let border_style = if playing {
        if selected {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::White)
        }
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let border_type = if selected {
        BorderType::Double
    } else {
        BorderType::Rounded
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .border_type(border_type);

    let bg_color = if sound.error_state {
        if selected {
            Color::LightRed
        } else {
            Color::Red
        }
    } else if playing {
        if selected {
            Color::Green
        } else {
            Color::Black
        }
    } else if selected {
        Color::Reset
    } else {
        Color::Reset
    };

    let icon = &sound.icon;

    let title_style = if playing {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let max_title_width = (area.width as usize).saturating_sub(4);

    let title_text = if sound.name.chars().count() > max_title_width {
        let spacer = "   ";
        let full_text: Vec<char> = format!("{}{}", sound.name, spacer).chars().collect();
        let len = full_text.len();
        let offset = (app.animation_offset as usize) % len;

        let mut scrolled = String::new();
        for i in 0..max_title_width {
            let idx = (offset + i) % len;
            scrolled.push(full_text[idx]);
        }
        scrolled
    } else {
        sound.name.clone()
    };

    let vol_width = (area.width as usize).saturating_sub(10);
    let knob_pos = if vol_width > 0 {
        (sound.volume_linear * (vol_width - 1) as f32).round() as usize
    } else {
        0
    };
    let mut slider = String::new();
    for i in 0..vol_width {
        if i == knob_pos {
            slider.push('●');
        } else if i < knob_pos {
            slider.push('━');
        } else {
            slider.push('─');
        }
    }

    let content = vec![
        Line::from(Span::raw(icon)),
        Line::from(Span::styled(title_text, title_style)),
        Line::from(vec![
            Span::styled(
                slider,
                Style::default().fg(if sound.error_state {
                    Color::Red
                } else if playing {
                    if selected {
                        Color::Black
                    } else {
                        Color::Green
                    }
                } else {
                    Color::Blue
                }),
            ),
            Span::raw(format!(" {:>3}%", (sound.volume_linear * 100.0) as u32)),
        ]),
    ];

    let p = Paragraph::new(content)
        .block(block.style(Style::default().bg(bg_color)))
        .alignment(Alignment::Center);

    f.render_widget(p, area);
}

