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

The workflow uploads:

- `codex-desktop-linux-x86_64.AppImage`
- `codex-desktop-linux-amd64.deb`
- `codex-desktop-linux-x86_64.rpm`
- `codex-desktop-linux-x86_64.pkg.tar.zst`
- matching `.sha256` checksum files
- `upstream-dmg-metadata.json`
- `patch-report.json`
- `build-info.json`

Release asset names stay short for readability in GitHub. Exact package
versions are stored in each native package and in `build-info.json`.

## Required Checks

Before release publication, the workflow must:

- rebuild `codex-app` from the selected DMG
- validate required upstream patches with
  `scripts/ci/validate-patch-report.js --profile upstream-build`
- inspect package contents for updater and update-builder payloads
- build packages with the same generated app bundle

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
