#!/usr/bin/env node

const fs = require("node:fs");

function parseArgs(argv) {
  const args = { architecture: "amd64", packageName: "chatgpt" };
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (!["--build-info", "--version", "--packages", "--architecture", "--package"].includes(key)) {
      throw new Error(`Unknown argument: ${key}`);
    }
    const value = argv[index + 1];
    if (!value) throw new Error(`Missing value for ${key}`);
    index += 1;
    if (key === "--build-info") args.buildInfo = value;
    if (key === "--version") args.version = value;
    if (key === "--packages") args.packages = value;
    if (key === "--architecture") args.architecture = value;
    if (key === "--package") args.packageName = value;
  }
  if (Boolean(args.buildInfo) === Boolean(args.version)) {
    throw new Error("Provide exactly one of --build-info or --version");
  }
  if (!args.packages) throw new Error("Missing --packages");
  return args;
}

function parseDebianStanzas(text) {
  return text
    .split(/\n\s*\n/)
    .map((block) => Object.fromEntries(block.split("\n").flatMap((line) => {
      const separator = line.indexOf(":");
      return separator > 0 ? [[line.slice(0, separator), line.slice(separator + 1).trim()]] : [];
    })));
}

function validateVersion({ buildInfo, version, packages, architecture, packageName }) {
  const dmgVersion = version ?? JSON.parse(fs.readFileSync(buildInfo, "utf8")).upstreamDmg?.appVersion;
  if (!dmgVersion) throw new Error("Could not read the DMG app version");

  const entry = parseDebianStanzas(fs.readFileSync(packages, "utf8")).find(
    (item) => item.Package === packageName && item.Architecture === architecture,
  );
  if (!entry?.Version) {
    throw new Error(`Could not find ${packageName}/${architecture} in ${packages}`);
  }
  if (dmgVersion !== entry.Version) {
    throw new Error(
      `Stale DMG edge detected: DMG contains ${dmgVersion}, official Linux repository advertises ${entry.Version}`,
    );
  }
  return dmgVersion;
}

if (require.main === module) {
  try {
    const version = validateVersion(parseArgs(process.argv.slice(2)));
    console.log(`Upstream DMG version ${version} matches the official Linux repository.`);
  } catch (error) {
    console.error(`ERROR: ${error.message}`);
    process.exitCode = 1;
  }
}

module.exports = { parseDebianStanzas, validateVersion };
