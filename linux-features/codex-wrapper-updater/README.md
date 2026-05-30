# codex-wrapper-updater

Optional Linux feature that adds a **separate** in-app update path for the
Codex Desktop Linux wrapper: this repository's Linux patches, bundled features,
packaging glue, launcher, and `codex-update-manager`.

This is intentionally distinct from the upstream Codex app update path. The
upstream path tracks the official macOS DMG. This feature tracks newer builds of
`codex-desktop-linux` itself.

## User-facing behavior

- Settings -> General shows:
  **Check for Codex Desktop Linux updates**.
- The setting is off by default.
- When it is on, `codex-update-manager` may check the wrapper repository for a
  newer Linux wrapper commit.
- If a newer wrapper build is available, a small top-right **Update** button is
  shown inside Codex.
- The button stays hidden when no wrapper update candidate is recorded.
- The button tooltip includes the recorded wrapper changelog when available.
- Clicking the button writes a pending marker and quits Codex. The feature hook
  applies the wrapper update while the app is stopped.

## Why this is a Linux feature

The wrapper updater is opt-in and lives under `linux-features/` because it is
not a required compatibility patch for every Linux build. Core only provides the
generic Linux feature loader and hook runner. This feature owns:

- the in-app wrapper update button;
- the Settings -> General runtime opt-in;
- the main-process bridge handler;
- the pending-update marker;
- the retry/apply hook;
- the updater command integration for wrapper checks and applies.

## Build-time opt-in

Add the feature id to the local feature config:

```json
{
  "enabled": ["codex-wrapper-updater"]
}
```

The file is `linux-features/features.json`, and it is intentionally gitignored.
After changing it, rebuild the app or package.

When enabled, the feature contributes three patch descriptors:

- `main-handler`: patches the Electron main bundle with the
  `codex-linux-wrapper-updater` bridge handler.
- `webview-runtime`: injects the webview runtime that creates and refreshes the
  top-right **Update** button.
- `settings-toggle`: patches the settings asset. Current upstream builds use
  `general-settings-*.js`; older builds may still use
  `keybinds-settings-linux.js`.

The feature also stages the same runtime hook twice:

- `.codex-linux/prelaunch.d/codex-wrapper-updater-apply-pending.sh`
- `.codex-linux/after-exit.d/codex-wrapper-updater-apply-pending.sh`

Both staged hooks call `apply-pending.sh`.

## Runtime opt-in

The Settings toggle persists this key:

```text
codex-linux-wrapper-updates-enabled
```

The setting is stored in the normal Linux app settings file:

```text
~/.config/<app-id>/settings.json
```

For the default app id, that is:

```text
~/.config/codex-desktop/settings.json
```

The setting is persisted through the app's `get-global-state` /
`set-global-state` path, not through the upstream typed settings schema. This is
important because `codex-linux-wrapper-updates-enabled` is a Linux-only key and
does not exist in upstream's settings schema.

`codex-update-manager` reads the same setting and treats it as the runtime opt-in
for wrapper update tracking. The static updater config still defaults wrapper
tracking to disabled, so existing installs keep their current DMG-only behavior.

## Detection flow

When wrapper updates are enabled, the app starts a best-effort background check:

```text
codex-update-manager check-wrapper
```

The command compares the installed wrapper metadata with the configured wrapper
remote/branch and records the result in:

```text
~/.local/state/codex-update-manager/state.json
```

The webview button is shown only when this state contains a non-empty
`candidate_wrapper_commit`.

Relevant state fields:

- `installed_wrapper_commit`
- `installed_wrapper_version`
- `candidate_wrapper_commit`
- `candidate_wrapper_version`
- `wrapper_changelog`

`check-wrapper --json` is useful for local inspection.

## Install/apply flow

Clicking the in-app **Update** button calls the main-process bridge action
`install`. The bridge:

1. resolves the current app state directory;
2. writes the pending marker;
3. exits Electron.

For the default app id, the marker path is:

```text
~/.local/state/codex-desktop/codex-wrapper-updater/pending
```

The feature hook then runs:

```text
codex-update-manager apply-wrapper-update
```

Apply behavior depends on the install type:

- **User-local install**: prefers `~/.local/bin/codex-desktop-update`, so it can
  update in place without privilege escalation.
- **Packaged install**: fetches the wrapper source, rebuilds a fresh native
  package from the cached/current DMG, and installs it with `pkexec`.

After a successful apply, the marker is removed, wrapper candidate fields are
cleared, and the app is relaunched by the after-exit hook.

## Failure and retry behavior

The hook is fail-closed:

- if `codex-update-manager` is missing, the marker is kept;
- if rebuild/install fails, the marker is kept;
- if required build tools are missing, the marker is kept;
- a lock directory prevents concurrent apply attempts;
- after a failed after-exit apply, relaunch uses
  `CODEX_WRAPPER_UPDATER_SKIP_PRELAUNCH_ONCE=1` so the next prelaunch hook does
  not immediately retry before the user sees the app again.

This means a failed update does not leave the app half-updated by the feature
hook. It leaves a retry marker for a later launch/exit.

## Local testing

Run the feature tests:

```bash
node --test linux-features/codex-wrapper-updater/test.js
```

Build and package with the feature enabled:

```bash
MAX_BUILD_THREADS=8 make build-app
MAX_BUILD_THREADS=8 make deb
```

Verify the installed build has the feature:

```bash
sed -n '1,160p' /opt/codex-desktop/resources/codex-linux-build-info.json
```

Verify the settings patch landed in the installed webview bundle:

```bash
rg "CodexLinuxWrapperUpdatesSetting|codex-linux-wrapper-updates-enabled|get-global-state|set-global-state" \
  /opt/codex-desktop/content/webview/assets/general-settings-*.js
```

Toggle the setting in Settings -> General, then verify:

```bash
rg "codex-linux-wrapper-updates-enabled" ~/.config/codex-desktop/settings.json
```

Inspect wrapper detection state:

```bash
codex-update-manager check-wrapper --json
codex-update-manager status --json
```

## Troubleshooting

If the row appears but the toggle immediately reverts, confirm the installed
settings bundle uses `get-global-state` and `set-global-state`. If it uses the
upstream typed settings API, the app will reject the Linux-only key.

If the **Update** button does not appear, check:

- the Settings toggle is on;
- `check-wrapper --json` records `candidate_wrapper_commit`;
- `~/.local/state/codex-update-manager/state.json` contains the candidate;
- the installed build includes `codex-wrapper-updater` in
  `codex-linux-build-info.json`.

If the app keeps retrying an update, inspect the pending marker and updater log:

```bash
ls -la ~/.local/state/codex-desktop/codex-wrapper-updater/
tail -n 200 ~/.local/state/codex-update-manager/service.log
```

Removing the pending marker stops retries, but normally the marker should be
left in place until the underlying apply problem is fixed.

## Known costs and risks

- Packaged wrapper updates are heavier than DMG checks because they rebuild a
  native Linux package locally.
- Packaged applies require `pkexec` and a graphical polkit authentication agent.
- Detection needs network access to inspect the configured wrapper remote.
- Missing build tools are reported as an apply failure; the marker is preserved
  for retry after tools are installed.
