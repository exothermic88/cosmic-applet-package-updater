# ncos Package Update Applet for COSMIC™

A package update applet for the COSMIC™ desktop on Arch Linux, packaged for the
ncos repo as `ncos-package-update-applet`. Fork of
[Ebbo/cosmic-applet-package-updater](https://github.com/Ebbo/cosmic-applet-package-updater).

Every check queries three update sources at once, and the popup shows each in
its own section with its own count, package list, and Update button:

- **Pacman** — official repos, via `checkupdates` (pacman-contrib)
- **AUR** — via `paru` or `yay` (auto-detected)
- **Flatpak** — if installed

![Main Interface](screenshots/Package-Updater-Main.png)

## Features

- **Frosted glass**: built against a current libcosmic, so the popup picks up
  the COSMIC 1.3 frosted-glass style when enabled in COSMIC Settings
- **Panel indicator**: icon plus combined update count across all three sources
- **Per-section updating**: each section has an Update button that opens your
  terminal with the right command (`sudo pacman -Syu` / `paru -Sua` or
  `yay -Sua` / `flatpak update`), plus an Update All button
- **Quick actions**: left-click opens the popup; middle-click the panel icon to
  run Update All directly
- **Automatic checking**: configurable interval (default 60 minutes), optional
  check on startup, automatic re-check after the update terminal closes
- **Instance sync**: multiple applet instances (panel + dock) stay in sync via
  a file watcher, with file-based locking so only one checks at a time

### Settings

- Check interval (1–1440 minutes)
- Auto-check on startup
- Include AUR updates (shown when paru/yay is installed)
- Include Flatpak updates (shown when flatpak is installed)
- Show update count in the panel
- Preferred terminal (default: `cosmic-term`)

## Installation

### From the ncos repo

```bash
sudo pacman -S ncos-package-update-applet
```

### Build the package locally

```bash
cd cosmic-applet-package-updater
makepkg -si
```

### Build from source

```bash
just build-release
sudo just install
```

After installing, add the applet to your panel in COSMIC Settings → Desktop →
Panel → Configure panel applets.

## Requirements

- COSMIC™ desktop (1.3+ for the frosted-glass style)
- `pacman-contrib` (provides `checkupdates`)
- Optional: `paru` or `yay` (AUR), `flatpak`
- Terminal emulator supporting the `-e` flag (default: `cosmic-term`)
- Building: Rust 1.93+, `just`

## Troubleshooting

- **"Update check already in progress"** — another instance holds the lock;
  wait a few seconds, or remove `$XDG_RUNTIME_DIR/cosmic-package-updater.lock`
- **Instances out of sync** — remove
  `$XDG_RUNTIME_DIR/cosmic-package-updater.sync` and restart the applet
- **Terminal not launching** — check the Preferred Terminal setting and that
  the terminal is installed

## License

GPL-3.0 — see [LICENSE](LICENSE).
