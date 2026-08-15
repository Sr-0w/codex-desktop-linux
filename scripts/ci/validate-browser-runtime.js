#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const MODERN_RUNTIME_MARKERS = [
  "codexLinuxBrowserUseEnvironmentShim",
  "codexLinuxOptionalResponseMetaHook",
];

function validateBrowserClient(clientPath) {
  if (!fs.existsSync(clientPath)) {
    throw new Error(`Bundled Browser Use client is missing: ${clientPath}`);
  }
  const source = fs.readFileSync(clientPath, "utf8");
  if (/\b(?:from\s*|import\s*(?:\(\s*)?)["'](?:node:)?process["']/.test(source)) {
    throw new Error(`Bundled Browser Use client still imports the blocked process module: ${clientPath}`);
  }
  if (source.includes("globalThis.nodeRepl?.env[")) {
    throw new Error(`Bundled Browser Use client has an unsafe nodeRepl env access: ${clientPath}`);
  }
  if (/return\s+[A-Za-z_$][\w$]*\.addAfterSubmittedCodeHook\s*\(/.test(source)) {
    throw new Error(`Bundled Browser Use client requires an optional response hook: ${clientPath}`);
  }
  if (source.includes("createElicitation.bind")) {
    for (const marker of MODERN_RUNTIME_MARKERS) {
      if (!source.includes(marker)) {
        throw new Error(`Bundled Browser Use client is missing ${marker}: ${clientPath}`);
      }
    }
    if (!/env:[A-Za-z_$][\w$]*\.env\?\?\{\}/.test(source)) {
      throw new Error(`Bundled Browser Use runtime clone does not preserve its safe env: ${clientPath}`);
    }
  }
}

function validateBundledBrowserRuntime(appDir) {
  const pluginsRoot = path.join(
    path.resolve(appDir),
    "resources",
    "plugins",
    "openai-bundled",
    "plugins",
  );
  for (const plugin of ["browser", "chrome"]) {
    validateBrowserClient(path.join(pluginsRoot, plugin, "scripts", "browser-client.mjs"));
  }
}

if (require.main === module) {
  const appDir = process.argv[2];
  if (!appDir) {
    process.stderr.write("Usage: validate-browser-runtime.js /path/to/codex-app\n");
    process.exit(2);
  }
  try {
    validateBundledBrowserRuntime(appDir);
    process.stdout.write("Bundled Browser Use runtime contract is valid.\n");
  } catch (error) {
    process.stderr.write(`ERROR: ${error.message}\n`);
    process.exit(1);
  }
}

module.exports = { MODERN_RUNTIME_MARKERS, validateBrowserClient, validateBundledBrowserRuntime };
