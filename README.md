# Codex Desktop for Linux

Unofficial Linux packaging and desktop-integration wrapper for OpenAI Codex
Desktop.

This project adapts the upstream Codex Desktop app into a Linux desktop
application and publishes ready-to-install Linux packages. It focuses on the
parts that make the app feel native on Linux: packaging, launcher behavior,
Wayland/X11 desktop integration, tray and warm-start handling, Browser Use
resources, Chrome native messaging, and Linux Computer Use support.

## What This Project Does

- Builds Linux packages from the upstream Codex Desktop app.
- Ships `.deb`, `.rpm`, pacman, and AppImage artifacts through GitHub Releases.
- Rebuilds native Electron modules for Linux.
- Adds Linux launcher behavior for desktop sessions, app identity, warm starts,
  local webview assets, and runtime paths.
- Stages Linux Browser Use, Chrome native messaging, and Computer Use support.
- Keeps optional Linux integrations isolated under `linux-features/`.

Server-side Codex features and model rollouts are still controlled by OpenAI
per account. This wrapper does not unlock account-gated functionality.

## Install

Download the latest package from
[GitHub Releases](https://github.com/Sr-0w/codex-desktop-linux/releases/latest).

| Platform | Artifact | Install command |
|---|---|---|
| Debian, Ubuntu, Pop!_OS, Mint | `codex-desktop_*.deb` | `sudo apt install ./codex-desktop_*.deb` |
| Fedora | `codex-desktop-*.rpm` | `sudo dnf install ./codex-desktop-*.rpm` |
| openSUSE | `codex-desktop-*.rpm` | `sudo zypper install ./codex-desktop-*.rpm` |
| Arch, Manjaro, EndeavourOS | `codex-desktop-*.pkg.tar.zst` | `sudo pacman -U ./codex-desktop-*.pkg.tar.zst` |
| Other Linux distros | `codex-desktop-*.AppImage` | `chmod +x ./codex-desktop-*.AppImage && ./codex-desktop-*.AppImage` |

Native packages install the app as `codex-desktop` and include the local update
manager. AppImage builds are portable and update manually by downloading a newer
release.

## After Install

Launch **Codex Desktop** from your app menu, or run:

```bash
codex-desktop
```

The Codex CLI is still required at runtime. The launcher can help install or
update `@openai/codex`, or you can manage the CLI yourself.

The generated app bundles a managed Linux Node.js runtime for its own Browser
Use and plugin resources, so normal users do not need to install Node.js just
to run the desktop app.

## Updates

Native packages include `codex-update-manager`, which can rebuild and install a
new local package when upstream Codex Desktop updates. AppImage users should
download the newest AppImage from Releases.

See [Updater](docs/updater.md) for update-manager details and rollback notes.

## Linux Integrations

Core Linux support includes:

- KDE, GNOME, and other desktop-session launcher behavior
- Wayland and X11 runtime handling
- tray and warm-start handoff
- Linux file-manager integration
- Browser Use availability on Linux
- Chrome, Chromium, Brave, and related native-host support
- Linux Computer Use backend registration

Optional integrations live in `linux-features/` and are disabled by default.
They are intended for advanced users and contributors who build from source.

## Build From Source

The public README intentionally stays focused on installing released packages.
Build and release-maintenance details live separately:

- [Build from source](docs/BUILD.md)
- [Build and packaging reference](docs/build-and-packaging.md)
- [Release workflow](.github/RELEASE.md)

## Project Docs

- [Troubleshooting](docs/troubleshooting.md)
- [Linux Computer Use](docs/linux-computer-use.md)
- [Native setup](docs/native-setup.md)
- [Nix](docs/nix.md)
- [Updater](docs/updater.md)
- [Linux Features architecture](docs/linux-features-architecture.md)
- [Architecture](docs/architecture.md)
- [Contributing](CONTRIBUTING.md)

## Disclaimer

This is an unofficial community project. Codex Desktop is a product of OpenAI.
This repository is not affiliated with or endorsed by OpenAI.

## License

MIT
