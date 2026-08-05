# KDE Native Window Integration

This opt-in feature keeps the primary Codex window integrated with KDE/KWin
decorations while giving the Codex pet dedicated overlay behavior on Plasma.

On KWin, the staged prelaunch hook installs a narrowly scoped rule matching the
Linux-only internal title `Codex Pet Overlay` and the current Codex application
id. The rule keeps that window borderless, above other windows, and out of the
taskbar, pager, and task switcher. It does not constrain position or size, so
Codex's own pet drag handling remains active.

The hook also loads a session-scoped KWin script before Electron starts. It
enforces the same policy for newly created pet windows and recognizes the
legacy `Codex` caption only for the pet's compact window geometry. This lets the
script apply the borderless and taskbar policy itself on older installed bundles
without accidentally applying it to the larger primary Codex window.

The hook uses `kreadconfig6`, `kwriteconfig6`, and `qdbus6`. It is idempotent,
does nothing outside a KWin session, and fails softly when KDE tools are not
available. It also removes border and task-switcher constraints from the older
broad `Codex Desktop KDE integration` rule when that exact legacy rule exists.

Enable `kde-native-corners` in `linux-features/features.json`, then rebuild the
app or a native package. The generated package and future updater rebuilds keep
the feature enabled through the normal Linux feature configuration bundle.
