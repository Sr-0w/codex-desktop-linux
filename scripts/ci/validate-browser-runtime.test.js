"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const {
  MODERN_RUNTIME_MARKERS,
  validateBundledBrowserRuntime,
} = require("./validate-browser-runtime.js");

function makeApp(clientSource) {
  const appDir = fs.mkdtempSync(path.join(os.tmpdir(), "codex-browser-runtime-"));
  for (const plugin of ["browser", "chrome"]) {
    const scriptsDir = path.join(
      appDir,
      "resources",
      "plugins",
      "openai-bundled",
      "plugins",
      plugin,
      "scripts",
    );
    fs.mkdirSync(scriptsDir, { recursive: true });
    fs.writeFileSync(path.join(scriptsDir, "browser-client.mjs"), clientSource);
  }
  return appDir;
}

function validClientSource() {
  return [
    ...MODERN_RUNTIME_MARKERS.map((marker) => `/*${marker}*/`),
    "const bridge={createElicitation:{bind(){}}};",
    "bridge.createElicitation.bind(bridge);",
    "const runtime={env:bridge.env??{}};",
  ].join("\n");
}

test("accepts Browser and Chrome clients with the secured runtime contract", () => {
  const appDir = makeApp(validClientSource());
  try {
    assert.doesNotThrow(() => validateBundledBrowserRuntime(appDir));
  } finally {
    fs.rmSync(appDir, { recursive: true, force: true });
  }
});

test("rejects a Browser client missing the safe environment shim", () => {
  const source = validClientSource().replace("/*codexLinuxBrowserUseEnvironmentShim*/", "");
  const appDir = makeApp(source);
  try {
    assert.throws(
      () => validateBundledBrowserRuntime(appDir),
      /codexLinuxBrowserUseEnvironmentShim/,
    );
  } finally {
    fs.rmSync(appDir, { recursive: true, force: true });
  }
});

test("rejects a Browser client importing node:process", () => {
  const appDir = makeApp(`${validClientSource()}\nimport { env } from "node:process";\n`);
  try {
    assert.throws(() => validateBundledBrowserRuntime(appDir), /blocked process module/);
  } finally {
    fs.rmSync(appDir, { recursive: true, force: true });
  }
});

test("accepts a legacy client that already guards optional runtime hooks", () => {
  const source = [
    "const environment=globalThis.nodeRepl?.env?.[\"KEY\"];",
    "function setupHook(bridge){",
    "  return typeof bridge.addAfterSubmittedCodeHook===\"function\"&&bridge.addAfterSubmittedCodeHook({}),{};",
    "}",
  ].join("\n");
  const appDir = makeApp(source);
  try {
    assert.doesNotThrow(() => validateBundledBrowserRuntime(appDir));
  } finally {
    fs.rmSync(appDir, { recursive: true, force: true });
  }
});

test("rejects an unsafe legacy nodeRepl environment access", () => {
  const appDir = makeApp("const value=globalThis.nodeRepl?.env[\"KEY\"];");
  try {
    assert.throws(() => validateBundledBrowserRuntime(appDir), /unsafe nodeRepl env access/);
  } finally {
    fs.rmSync(appDir, { recursive: true, force: true });
  }
});
