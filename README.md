# Tanin

A simple, focused TUI ambient noise generator written in Rust. Focus, relax, or sleep with your favorite background sounds directly from your terminal.

Inspired by [Blanket](https://github.com/rafaelmardojai/blanket).


https://github.com/user-attachments/assets/2cbfa09f-52fd-4816-939c-ed92435fa605

## Features

*   **TUI Interface**: Fast, keyboard-centric interface built with Ratatui (mouse is also supported (: ).
*   **Audio Mixing**: Play multiple sounds simultaneously with individual volume controls.
*   **Custom Sounds**: Built-in support for downloading sounds from YouTube and other sources via `yt-dlp`.
*   **Presets**




## Installation

### From Package Managers

```bash
cargo install tanin    # compiles from source
cargo binstall tanin   # downloads a prebuilt binary
```

**Arch Linux (AUR):**

```bash
yay -S tanin-bin   # prebuilt binary
yay -S tanin-git   # latest git revision
```

### Nix / NixOS

The flake exposes two packages: `tanin` compiles from source and `tanin-bin`
unpacks the prebuilt release archive.

```bash
nix run github:AnonMiraj/Tanin#tanin-bin
```

```nix
# flake.nix
inputs.tanin.url = "github:AnonMiraj/Tanin";

# configuration.nix
environment.systemPackages = [
  inputs.tanin.packages.${pkgs.stdenv.hostPlatform.system}.tanin-bin
  # or: inputs.tanin.packages.${pkgs.stdenv.hostPlatform.system}.tanin
];
```

<details>
<summary>Manual installation (build from source)</summary>

Requires a Rust toolchain plus the system libraries below.

| Distribution | Command |
| --- | --- |
| Debian / Ubuntu | `sudo apt install pkg-config libasound2-dev libjack-jackd2-dev libssl-dev libopus-dev` |
| Arch Linux | `sudo pacman -S --needed pkgconf alsa-lib jack2 openssl opus` |
| Fedora | `sudo dnf install pkgconf-pkg-config alsa-lib-devel jack-audio-connection-kit-devel openssl-devel opus-devel` |
| macOS | `brew install pkg-config opus jack` |

```bash
git clone https://github.com/AnonMiraj/Tanin.git
cd Tanin
cargo build --release
sudo install -Dm755 target/release/tanin /usr/local/bin/tanin
```

Running `cargo run --release` from the repository root picks up the bundled
sounds in `assets/` directly, with nothing to download. After a system-wide
install, copy them to the path the binary looks for:

```bash
sudo install -d /usr/share/tanin/assets
sudo cp -r assets/sounds.toml assets/sounds /usr/share/tanin/assets/
```

Otherwise Tanin downloads them into your data directory
(`~/.local/share/tanin/`) on first launch.

</details>

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
