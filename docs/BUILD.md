# Build From Source

Most users should install a package from
[GitHub Releases](https://github.com/Sr-0w/codex-desktop-linux/releases/latest).
Use this guide when you want to develop the wrapper, enable optional Linux
features, or reproduce release builds locally.

## Prerequisites

The build needs:

- `python3`
- `7z` or `7zz`
- `curl`
- `unzip`
- `make`
- `g++`
- Rust with `cargo`

Bootstrap common dependencies on supported distros:

```bash
bash scripts/install-deps.sh
```

## Build The App

```bash
git clone https://github.com/Sr-0w/codex-desktop-linux.git
cd codex-desktop-linux
make build-app
```

Use a local DMG:

```bash
make build-app DMG=/path/to/Codex.dmg
```

Run the generated app:

```bash
make run-app
```

## Build Packages

Package scripts use the already-generated `codex-app/` directory.

```bash
make deb
make rpm
make pacman
make appimage
```

Or let the repo choose the native package format for the host:

```bash
make package
```

See [Build and packaging](build-and-packaging.md) for full package-builder
details, version overrides, AppImage notes, Electron mirrors, and CI hints.

## Optional Linux Features

Optional features are disabled by default. To enable tracked features before
building:

```bash
cp linux-features/features.example.json linux-features/features.json
```

Edit `linux-features/features.json`, then rebuild:

```bash
make build-app
make package
```

Feature contract:

- [Linux Features](../linux-features/README.md)
- [Linux Features architecture](linux-features-architecture.md)

## Local Validation

```bash
node --test scripts/patch-linux-window-ui.test.js
node --test linux-features/*/test.js
bash tests/scripts_smoke.sh
cargo test --workspace --all-targets
```

With Docker or Podman:

```bash
CI_CONTAINER_ENGINE=podman ./scripts/ci-local.sh pr
```
