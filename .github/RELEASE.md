# Release Workflow

This repository publishes Linux packages through GitHub Actions. The release
workflow builds the Linux app from the upstream `Codex.dmg`, validates required
patches, packages the generated app, uploads release artifacts, and creates or
updates a GitHub Release.

## Workflow

Run **Release Artifacts** from the Actions tab, or push a tag that starts with
`v`.

Manual inputs:

- `tag`: release tag to create or update, for example `v2026.06.30.150000`.
  When omitted, the workflow creates a UTC timestamp tag.
- `package_version`: native package version. When omitted, the workflow uses
  the tag without `v` plus the short commit hash.
- `upstream_dmg_url`: upstream Codex Desktop DMG URL.
- `draft`: create the GitHub Release as a draft.
- `prerelease`: mark the GitHub Release as a prerelease.

## Artifacts

The workflow builds independently on native `x86_64` and `aarch64` runners and
uploads:

- `codex-desktop-linux-x86_64.AppImage`
- `codex-desktop-linux-aarch64.AppImage`
- `codex-desktop-linux-amd64.deb`
- `codex-desktop-linux-arm64.deb`
- `codex-desktop-linux-x86_64.rpm`
- `codex-desktop-linux-aarch64.rpm`
- `codex-desktop-linux-x86_64.pkg.tar.zst`
- `codex-desktop-linux-aarch64.pkg.tar.zst`
- `codex-desktop-linux-amd64.gentoo.tar.zst`
- `codex-desktop-linux-arm64.gentoo.tar.zst`
- matching `.sha256` checksum files
- architecture-suffixed upstream DMG, patch report, and build-info metadata

Release asset names stay short for readability in GitHub. Exact package
versions are stored in each native package and in `build-info.json`.

## Required Checks

Before release publication, the workflow must:

- rebuild `codex-app` from the selected DMG
- build the complete app independently on native x86_64 and ARM64 runners
- validate Electron, Node.js, native addons, and bundled helper ELF architecture
  with `scripts/ci/validate-app-architecture.sh`
- validate required upstream patches with
  `scripts/ci/validate-patch-report.js --profile upstream-build`
- inspect package contents for updater and update-builder payloads
- build packages with the same generated app bundle

The official primary-runtime archive used for Browser Use currently contains
only a Linux x86_64 `node_repl`. When OpenAI publishes an ARM64 archive, set
the repository variables `CODEX_BROWSER_USE_NODE_REPL_ARM64_URL` and
`CODEX_BROWSER_USE_NODE_REPL_ARM64_SHA256`; ARM64 builds then validate and
stage it through the same installer path. Until then, ARM64 releases are built
without that privileged Browser Use runtime.

## Local Dry Run

Use this for local confidence before pushing a tag:

```bash
node --test scripts/patch-linux-window-ui.test.js
node --test linux-features/*/test.js
bash tests/scripts_smoke.sh
cargo test --workspace --all-targets
```

On a machine with Docker or Podman:

```bash
CI_CONTAINER_ENGINE=podman ./scripts/ci-local.sh pr
```

## Release Policy

Keep release jobs green on a clean checkout before publishing a non-draft
release. If a Linux feature is local-only under `linux-features/local/`, it is
not included in public release artifacts unless it is promoted into the tracked
`linux-features/` tree.
