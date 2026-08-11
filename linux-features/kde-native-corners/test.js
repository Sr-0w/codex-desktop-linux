"use strict";

const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const { patches, applyMainBundlePatch } = require("./patch.js");
const {
  applyFramelessTitlebarMainPatch,
} = require("../frameless-titlebar/patch.js");

test("uses native KDE decorations only for the current primary window", () => {
  const source =
    "function R9({appearance:e,platform:n}){switch(e){case`quickChat`:case`primary`:return n===`linux`?{titleBarStyle:`hidden`,...e===`quickChat`?{resizable:!0}:{}}:{titleBarStyle:`default`};case`secondary`:return{titleBarStyle:`default`}}}";
  const patched = applyMainBundlePatch(applyMainBundlePatch(source));

  assert.match(
    patched,
    /n===`linux`\?e===`primary`\?\{titleBarStyle:`default`\}:\{titleBarStyle:`hidden`,resizable:!0\}/,
  );
});

test("runs after core titlebar patching and before frameless cleanup", () => {
  assert.equal(patches[0].order, 20_710);

  const source =
    "function z9({appearance:e,platform:n,windowZoom:r=1}){switch(e){case`quickChat`:case`primary`:return n===`darwin`?{titleBarStyle:`hiddenInset`}:n===`win32`?{titleBarStyle:`hidden`,titleBarOverlay:j9(r),...e===`quickChat`?{resizable:!0}:{}}:n===`linux`?{titleBarStyle:`hidden`,titleBarOverlay:codexLinuxTitleBarOverlay(r),...e===`quickChat`?{resizable:!0}:{}}:{titleBarStyle:`default`,...e===`quickChat`?{resizable:!0}:{}};}}";
  const kdePatched = applyMainBundlePatch(source);
  const combined = applyFramelessTitlebarMainPatch(kdePatched);

  assert.match(
    combined,
    /n===`linux`\?e===`primary`\?\{titleBarStyle:`default`\}:\{titleBarStyle:`hidden`,resizable:!0\}/,
  );
  assert.doesNotMatch(combined, /titleBarOverlay:codexLinuxTitleBarOverlay/);
});

test("stages a KWin rule that is overlay-only and leaves movement unconstrained", () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codex-kde-rule-"));
  const binDir = path.join(tempRoot, "bin");
  const logPath = path.join(tempRoot, "calls.log");
  fs.mkdirSync(binDir);

  const fakeRead = `#!/bin/sh
group=""; key=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --group) group="$2"; shift 2 ;;
    --key) key="$2"; shift 2 ;;
    *) shift ;;
  esac
done
case "$group:$key" in
  General:rules) printf '%s\\n' 'codex-desktop,2,3' ;;
  codex-desktop:Description) printf '%s\\n' 'Codex Desktop KDE integration' ;;
esac
`;
  const fakeWrite = `#!/bin/sh
printf '%s\\n' "$*" >> "$KWIN_TEST_LOG"
`;
  const fakeDbus = `#!/bin/sh
printf 'qdbus %s\\n' "$*" >> "$KWIN_TEST_LOG"
`;
  for (const [name, source] of [
    ["kreadconfig6", fakeRead],
    ["kwriteconfig6", fakeWrite],
    ["qdbus6", fakeDbus],
  ]) {
    const target = path.join(binDir, name);
    fs.writeFileSync(target, source, { mode: 0o755 });
  }

  const result = spawnSync("bash", [path.join(__dirname, "configure-kwin.sh")], {
    encoding: "utf8",
    env: {
      ...process.env,
      CODEX_LINUX_APP_ID: "codex-desktop",
      KWIN_TEST_LOG: logPath,
      PATH: `${binDir}:${process.env.PATH}`,
      XDG_CURRENT_DESKTOP: "KDE",
      XDG_RUNTIME_DIR: path.join(tempRoot, "runtime"),
    },
  });
  assert.equal(result.status, 0, result.stderr);

  const calls = fs.readFileSync(logPath, "utf8");
  assert.match(calls, /--group General --key rules codex-desktop,2,3,codex-pet-overlay-codex-desktop/);
  assert.match(calls, /--group codex-pet-overlay-codex-desktop --key title Codex Pet Overlay/);
  assert.match(calls, /--key noborder --type bool true/);
  assert.match(calls, /--key above --type bool true/);
  assert.match(calls, /--key skiptaskbar --type bool true/);
  assert.doesNotMatch(calls, /--key (?:position|positionrule|size|sizerule)\b/);
  assert.match(calls, /--group codex-desktop --key noborderrule --delete/);
  assert.match(calls, /qdbus org\.kde\.KWin \/KWin org\.kde\.KWin\.reconfigure/);
  assert.match(
    calls,
    /qdbus org\.kde\.KWin \/Scripting org\.kde\.kwin\.Scripting\.loadScript .*pet-overlay-codex-desktop\.js codex-pet-overlay-live-codex-desktop/,
  );
  assert.match(
    calls,
    /qdbus org\.kde\.KWin \/Scripting org\.kde\.kwin\.Scripting\.start/,
  );

  const liveScript = fs.readFileSync(
    path.join(
      tempRoot,
      "runtime",
      "codex-desktop-linux",
      "kwin",
      "pet-overlay-codex-desktop.js",
    ),
    "utf8",
  );
  assert.match(liveScript, /caption === "Codex Pet Overlay"/);
  assert.match(liveScript, /Number\(geometry\.width\) <= 512/);
  assert.match(liveScript, /window\.keepAbove = true/);
  assert.match(liveScript, /workspace\.windowAdded\.connect\(codexWatchWindow\)/);

  const signal = () => ({ connect() {} });
  const windowFixture = (overrides) => ({
    caption: "Codex",
    resourceClass: "codex-desktop",
    normalWindow: true,
    frameGeometry: { width: 800, height: 600 },
    keepAbove: false,
    noBorder: false,
    skipTaskbar: false,
    skipPager: false,
    skipSwitcher: false,
    captionChanged: signal(),
    frameGeometryChanged: signal(),
    skipTaskbarChanged: signal(),
    noBorderChanged: signal(),
    ...overrides,
  });
  const primaryWindow = windowFixture({});
  const legacyPetWindow = windowFixture({
    frameGeometry: { width: 408, height: 400 },
  });
  const titledPetWindow = windowFixture({
    caption: "Codex Pet Overlay",
    frameGeometry: { width: 700, height: 600 },
  });

  vm.runInNewContext(liveScript, {
    workspace: {
      windowAdded: signal(),
      windowList: () => [primaryWindow, legacyPetWindow, titledPetWindow],
    },
  });

  assert.equal(primaryWindow.keepAbove, false);
  for (const petWindow of [legacyPetWindow, titledPetWindow]) {
    assert.equal(petWindow.keepAbove, true);
    assert.equal(petWindow.noBorder, true);
    assert.equal(petWindow.skipTaskbar, true);
    assert.equal(petWindow.skipPager, true);
    assert.equal(petWindow.skipSwitcher, true);
  }
});
