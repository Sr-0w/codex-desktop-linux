"use strict";

function applyMainBundlePatch(source) {
  const legacyPrimaryWindowOptions =
    "n===`linux`?{titleBarStyle:`hidden`,titleBarOverlay:codexLinuxTitleBarOverlay(r)}:{titleBarStyle:`default`}";
  const nativePrimaryWindowOptions =
    "n===`linux`?{titleBarStyle:`default`}:{titleBarStyle:`default`}";

  let patched = source;
  if (patched.includes(legacyPrimaryWindowOptions)) {
    patched = patched.replace(legacyPrimaryWindowOptions, nativePrimaryWindowOptions);
  } else {
    const currentPrimaryWindowOptions =
      /([A-Za-z_$][\w$]*)===`linux`\?\{titleBarStyle:`hidden`,(?:titleBarOverlay:codexLinuxTitleBarOverlay\([^)]*\),)?\.\.\.([A-Za-z_$][\w$]*)===`quickChat`\?\{resizable:!0\}:\{\}\}/;
    const currentMatch = patched.match(currentPrimaryWindowOptions);
    if (currentMatch != null) {
      const platformVar = currentMatch[1];
      const appearanceVar = currentMatch[2];
      patched = patched.replace(
        currentPrimaryWindowOptions,
        `${platformVar}===\`linux\`?${appearanceVar}===\`primary\`?{titleBarStyle:\`default\`}:{titleBarStyle:\`hidden\`,resizable:!0}`,
      );
    } else if (
      !/===`linux`\?[A-Za-z_$][\w$]*===`primary`\?\{titleBarStyle:`default`\}/.test(
        patched,
      )
    ) {
      console.warn("WARN: KDE native primary window options target not found");
    }
  }

  const legacyZoomOverlay =
    "process.platform===`darwin`?n.setWindowButtonPosition(p9(t)):(process.platform===`win32`||process.platform===`linux`)&&(this.windowZooms.set(n.id,t),n.setTitleBarOverlay(process.platform===`linux`?codexLinuxTitleBarOverlay(t):m9(t)))";
  const windowsOnlyZoomOverlay =
    "process.platform===`darwin`?n.setWindowButtonPosition(p9(t)):process.platform===`win32`&&(this.windowZooms.set(n.id,t),n.setTitleBarOverlay(m9(t)))";
  const legacyThemeOverlayHook =
    "installApplicationMenuTitleBarOverlaySync(e,t){if((process.platform!==`win32`&&process.platform!==`linux`)||t!==`primary`)return;let n=()=>{e.isDestroyed()||e.setTitleBarOverlay(process.platform===`linux`?codexLinuxTitleBarOverlay(this.windowZooms.get(e.id)):m9(this.windowZooms.get(e.id)))};return a.nativeTheme.on(`updated`,n),n(),()=>{a.nativeTheme.off(`updated`,n)}}";
  const windowsOnlyThemeOverlayHook =
    "installApplicationMenuTitleBarOverlaySync(e,t){if(process.platform!==`win32`||t!==`primary`)return;let n=()=>{e.isDestroyed()||e.setTitleBarOverlay(m9(this.windowZooms.get(e.id)))};return a.nativeTheme.on(`updated`,n),n(),()=>{a.nativeTheme.off(`updated`,n)}}";

  patched = patched
    .replace(legacyZoomOverlay, windowsOnlyZoomOverlay)
    .replace(legacyThemeOverlayHook, windowsOnlyThemeOverlayHook);
  return patched;
}

module.exports = {
  patches: [
    {
      id: "main-process",
      phase: "main-bundle",
      order: 20_710,
      ciPolicy: "optional",
      apply: applyMainBundlePatch,
    },
  ],
  applyMainBundlePatch,
};
