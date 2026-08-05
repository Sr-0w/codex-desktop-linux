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
- Ships `.deb`, `.rpm`, pacman, Gentoo overlay, postmarketOS APK, and AppImage
  artifacts through GitHub Releases.
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

Choose `amd64`/`x86_64` for Intel or AMD systems and `arm64`/`aarch64` for
64-bit ARM systems such as a Raspberry Pi 4 running a 64-bit OS.

| Platform | Artifact pattern | Install command |
|---|---|---|
| Debian, Ubuntu, Raspberry Pi OS, Pop!_OS, Mint | `codex-desktop-linux-{amd64,arm64}.deb` | `sudo apt install ./codex-desktop-linux-<arch>.deb` |
| Fedora | `codex-desktop-linux-{x86_64,aarch64}.rpm` | `sudo dnf install ./codex-desktop-linux-<arch>.rpm` |
| openSUSE | `codex-desktop-linux-{x86_64,aarch64}.rpm` | `sudo zypper install ./codex-desktop-linux-<arch>.rpm` |
| Arch, Manjaro, EndeavourOS, Arch Linux ARM | `codex-desktop-linux-{x86_64,aarch64}.pkg.tar.zst` | `sudo pacman -U ./codex-desktop-linux-<arch>.pkg.tar.zst` |
| Gentoo | `codex-desktop-linux-{amd64,arm64}.gentoo.tar.zst` | `tar -xf codex-desktop-linux-<arch>.gentoo.tar.zst && sudo ./codex-desktop-linux-gentoo/install-gentoo.sh` |
| postmarketOS with Plasma Mobile | `codex-desktop-linux-postmarketos-aarch64.apk` | `doas apk add --allow-untrusted --upgrade ./codex-desktop-linux-postmarketos-aarch64.apk` |
| Other 64-bit glibc distros | `codex-desktop-linux-{x86_64,aarch64}.AppImage` | `chmod +x ./codex-desktop-linux-<arch>.AppImage && ./codex-desktop-linux-<arch>.AppImage` |

For postmarketOS, download the adjacent `.sha256` file and verify it before the
first install. Use `sudo` instead of `doas` when that is the configured privilege
tool:

```bash
sha256sum -c codex-desktop-linux-postmarketos-aarch64.apk.sha256
doas apk add --allow-untrusted --upgrade ./codex-desktop-linux-postmarketos-aarch64.apk
```

Native packages install the app as `codex-desktop` and include
`codex-update-manager`. The Gentoo release installs
`app-editors/codex-desktop-bin` from a local Portage overlay. The postmarketOS
package uses native Wayland integration, Wayland text input, and a private
glibc/Mesa runtime with the Raspberry Pi 4 V3D driver; it does not replace the
system musl C library. AppImage builds are portable, check GitHub Releases on
launch, and can replace themselves after confirmation on compatible glibc
distributions when a newer release is published.

ARM64 packages currently include the app, CLI integration, native updater,
Chrome native host, and Linux Computer Use binaries. OpenAI's separately
published privileged Browser Use `node_repl` runtime is currently available to
this project only for Linux x86_64. Browser Use on ARM64 remains unavailable
unless a verified ARM64 runtime is supplied during the build. ARM 32-bit is not
a release target. The dedicated postmarketOS APK is the supported musl-system
exception; generic Alpine desktop configurations are not part of its release
test matrix.

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

Debian, RPM, pacman, and Gentoo packages include `codex-update-manager`, which
can rebuild and install a new local package when upstream Codex Desktop updates.
On postmarketOS, the manager downloads the fixed ARM64 APK asset from the latest
stable GitHub Release, verifies its published SHA-256 and package identity, and
offers to install it after Codex exits. AppImage users get a release prompt when
a newer matching-architecture AppImage is published; the previous file remains
available as a timestamped backup.

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

The wrapper source code in this repository is MIT licensed.

Released packages are built from upstream Codex Desktop. OpenAI-owned app
assets, trademarks, services, and account-gated features remain under their own
terms.
