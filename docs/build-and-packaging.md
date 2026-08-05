# Build And Packaging

## Prerequisites

You need:

- `python3`, `7z` or `7zz`, `curl`, `unzip`, `tar`, `make`, `g++`
- Rust toolchain with `cargo` for `codex-update-manager`,
  `codex-computer-use-linux`, the Chrome extension host binary, and optional
  Rust-backed features such as Read Aloud MCP and Record & Replay

The installer downloads a managed Linux Node.js runtime into
`codex-app/resources/node-runtime` and uses it for `node`, `npm`, and `npx`
during the build. Existing `nvm`, asdf, Volta, NodeSource, or nodejs.org
installs are fine, but no longer required for the generated app build. The
dependency helper may still install or validate a distro Node.js toolchain on
some bootstrap paths.

Bootstrap dependencies:

```bash
bash scripts/install-deps.sh
```

It detects `apt`, `dnf5`, `dnf`, `pacman`, or `zypper`, installs system
packages, and bootstraps Rust through `rustup` when needed. Gentoo hosts use
`emerge` for system packages.

## Manual Dependencies

```bash
# Fedora 41+
sudo dnf install python3 7zip curl unzip tar rpm-build make gcc-c++ @development-tools

# Fedora < 41
sudo dnf install python3 p7zip p7zip-plugins curl unzip tar rpm-build make gcc-c++
sudo dnf groupinstall 'Development Tools'

# openSUSE
sudo zypper install python3 p7zip-full curl unzip tar
sudo zypper install -t pattern devel_basis

# Arch / Manjaro
sudo pacman -S --needed python p7zip curl unzip tar zstd base-devel

# Gentoo
sudo emerge --ask app-arch/7zip app-arch/unzip app-arch/zstd dev-lang/python net-misc/curl sys-devel/gcc sys-devel/make

# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On apt-based systems, `scripts/install-deps.sh` can still bootstrap optional
NodeSource Node.js for users who want a system Node.js toolchain:

```bash
bash scripts/install-deps.sh
NODEJS_MAJOR=24 bash scripts/install-deps.sh
```

Ubuntu-family `p7zip-full` can be too old for newer APFS DMGs, so
`install-deps.sh` bootstraps `7zz` into `~/.local/bin` by default.

## Generate The Local App

```bash
make build-app
make build-app-fresh
make build-app DMG=/path/to/Codex.dmg
```

Equivalent direct commands:

```bash
./install.sh
./install.sh /path/to/Codex.dmg
./install.sh --fresh
```

The default path stores upstream DMG headers, plus a hash of the upstream URL,
next to `Codex.dmg` and refreshes the cached file when that upstream fingerprint
changes. `--fresh` still forces a cache removal before rebuilding, and an
explicit `DMG=/path/to/Codex.dmg` uses that file exactly.
Native install shortcuts use `--fresh --reuse-dmg`, so they clean the generated
app directory while still reusing the cached DMG when upstream metadata matches.

## Architectures

The installer supports native `x86_64` and `aarch64` builds. Build on the target
architecture: Electron, the managed Node.js runtime, Electron addons, Rust
helpers, and the updater are all selected or compiled for the host. The release
workflow uses separate native GitHub runners and never repackages an x86_64
`codex-app` as ARM64.

On ARM64, Raspberry Pi OS 64-bit, Debian arm64, Ubuntu arm64,
Fedora/openSUSE aarch64, Arch Linux ARM, Gentoo arm64, NixOS aarch64, and the
dedicated postmarketOS/Plasma Mobile APK are release targets. Generic package
formats and both AppImages target glibc. The postmarketOS APK instead carries an
app-local glibc dependency closure and private Mesa DRI drivers while keeping
the host musl system unchanged. ARM 32-bit is outside the release contract.

OpenAI currently publishes the privileged Browser Use `node_repl` primary
runtime used here only for Linux x86_64. An ARM64 build can supply a verified
archive explicitly:

```bash
CODEX_BROWSER_USE_NODE_REPL_RUNTIME_URL=https://example.invalid/runtime-arm64.tar.xz \
CODEX_BROWSER_USE_NODE_REPL_RUNTIME_SHA256=<sha256> \
./install.sh
```

The archive must contain
`codex-primary-runtime/dependencies/bin/node_repl`; its ELF architecture and
checksum are validated before staging.

Run the generated app:

```bash
make run-app
./codex-app/start.sh
```

## Running The Generated App

By default, second launches reuse the running app through the Linux warm-start
handoff.

Open an independent app process:

```bash
./codex-app/start.sh --new-instance
```

Configure the port range or make every launch use multi-instance mode:

```bash
CODEX_MULTI_LAUNCH_PORT_RANGE=5175-5199 ./codex-app/start.sh --new-instance
CODEX_MULTI_LAUNCH=1 CODEX_MULTI_LAUNCH_PORT_RANGE=5175-5199 ./codex-app/start.sh
```

## Package Formats

After `make build-app` or `make build-app-fresh`, build a package from
`codex-app/`:

| Format | Build command | Output | Install |
|---|---|---|---|
| Debian | `make deb` | `dist/codex-desktop_*.deb` | `sudo dpkg -i dist/codex-desktop_*.deb` |
| RPM | `make rpm` | `dist/codex-desktop-*.{x86_64,aarch64}.rpm` | `sudo dnf install dist/codex-desktop-*.rpm` or `sudo zypper install dist/codex-desktop-*.rpm` |
| Arch | `make pacman` | `dist/codex-desktop-*.pkg.tar.zst` | `sudo pacman -U dist/codex-desktop-*.pkg.tar.zst` |
| Gentoo | `make gentoo` | `dist/codex-desktop-*.gentoo.tar.zst` | `sudo target/release/codex-update-manager install-gentoo --path dist/codex-desktop-*.gentoo.tar.zst` |
| postmarketOS | `make postmarketos` | `dist/codex-desktop-*-aarch64.apk` | `doas apk add --allow-untrusted --upgrade dist/codex-desktop-*-aarch64.apk` |
| AppImage | `make appimage` | `dist/codex-desktop-*.AppImage` | Run directly |
| Auto-detect (glibc native formats) | `make package && make install` | matches Debian, RPM, Arch, or Gentoo host | handled by `make install` |

Override package version:

```bash
PACKAGE_VERSION=2026.03.24.220723+88f07cd3 make deb
```

The packaging scripts only repackage what is already in `codex-app/`; they do
not download or extract the DMG.

The Gentoo builder creates a self-contained Portage overlay artifact for
`app-editors/codex-desktop-bin`. It contains the overlay, Manifest, distfile,
and `install-gentoo.sh`. The installed payload omits the `systemd --user` unit;
OpenRC/non-systemd sessions rely on the packaged launch-time
`codex-update-manager check-now --if-stale` fallback.

### postmarketOS / Plasma Mobile

The APK must be assembled on a native aarch64 glibc build host, as done by the
release workflow on `ubuntu-24.04-arm`. Generate `codex-app/` there with the
postmarketOS target overrides, install `patchelf`, `pax-utils`, Mesa DRI files,
Alpine `abuild` through Docker/Podman, `musl-tools`, and the Rust
`aarch64-unknown-linux-musl` target, then run:

```bash
make postmarketos
```

`make postmarketos` builds a musl-linked updater, stages the private glibc
runtime and V3D driver, and asks Alpine 3.22 `abuild` to create the APK. Building
the app itself directly on postmarketOS is not supported because Electron and
its native addons must first be produced together against glibc. The release
workflow performs an Alpine-musl smoke test of Electron-as-Node, managed Node,
and `better-sqlite3` before publishing the package.

## AppImage Local Self-Build

```bash
make build-app
make appimage
./dist/codex-desktop-*.AppImage
```

The AppImage flow does not include `codex-update-manager`, the systemd user
service, polkit policy, or the native-package update builder. The AppImage
runtime checks GitHub Releases and prompts the user to download and install a
newer AppImage after confirmation. The previous AppImage is kept as a
timestamped backup next to the installed file.

When upstream Codex Desktop changes:

```bash
git pull --ff-only
make build-app-fresh
make appimage
```

AppImage builds require `appimagetool` on `PATH`, or:

```bash
APPIMAGETOOL=/path/to/appimagetool make appimage
```

## Electron Mirrors

If runtime downloads from GitHub are slow or blocked:

```bash
ELECTRON_MIRROR=https://npmmirror.com/mirrors/electron/ make build-app
```

`ELECTRON_HEADERS_URL` is passed to `@electron/rebuild --dist-url` and must
provide both `node-v<version>-headers.tar.gz` and the matching `SHASUMS256.txt`.

## Build Parallelism

```bash
MAX_BUILD_THREADS=8 make build-app-fresh
MAX_BUILD_THREADS=8 make package
MAX_BUILD_THREADS=8 make install-native
```

`MAX_BUILD_THREADS=0` is the default and preserves each tool's automatic
behavior. A nonzero value controls Cargo jobs, native module rebuild jobs,
Debian package compression, pacman package compression, and RPM zstd payload
compression.

## Make Targets

Run:

```bash
make help
```

Common targets:

```bash
make check
make test
make build-updater
make build-app
make build-app-fresh
make bootstrap-native
make install-native
make update-native
make run-app
make build-dev-app
make run-dev-app
make deb
make rpm
make pacman
make appimage
make package
make install
make service-enable
make service-status
make clean-dist
make clean-state
```
