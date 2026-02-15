# Tanin

A simple, focused TUI ambient noise generator written in Rust. Focus, relax, or sleep with your favorite background sounds directly from your terminal.

Inspired by [Blanket](https://github.com/rafaelmardojai/blanket).

## Features

*   **TUI Interface**: Fast, keyboard-centric interface built with Ratatui (mouse is also supported (: ).
*   **Audio Mixing**: Play multiple sounds simultaneously with individual volume controls.
*   **Custom Sounds**: Built-in support for downloading sounds from YouTube and other sources via `yt-dlp`.
*   **Presets**

## Installation

### From Package Managers

**Cargo:**
```bash
cargo install tanin
```

**Arch Linux (AUR):**
```bash
yay -S tanin
# or using paru
paru -S tanin
```

### Build from Source
```bash
git clone https://github.com/AnonMiraj/Tanin.git
cd Tanin
cargo run --release
```

### Requirements
*   **Optional**: [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) (for downloading custom sounds)

## Configuration & Custom Sounds

Tanin stores configuration in your system's standard config directory (e.g., `~/.config/tanin/` on Linux).

### Adding Sounds Manually
You can add custom sounds by editing `sounds.toml` in your configuration directory. Use the format `[Category.Sound_NAME]` to group sounds. Tanin will automatically download sounds if a url is provided.

**Example:**
```toml
[idk.1_Hour_of_Silence_Occasionally_Broken_up_by_a_Metal_Pipe_Falling_Sound_Effect]
file = "~/.local/share/tanin/sounds/An_hour_of_metal_pipes.mp3" # auto added if url is provided
url = "https://www.youtube.com/watch?v=YmHZI03a_Yo"
icon = "🎵" # Optional
```

### Configuration (`config.toml`)
The `config.toml` file handles general application settings:
*   **`general.hidden_categories`**: List of categories to hide from the view.
*   **`general.category_order`**: Define the sort order of categories.
*   **`sounds.<id>.hidden`**: Hide specific sounds.
