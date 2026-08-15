#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const repoRoot = path.resolve(__dirname, "..", "..");
const featuresRoot = path.join(repoRoot, "linux-features");
const {
  discoverLinuxFeatureManifests,
  enabledLinuxFeatureInstallPlan,
  enabledLinuxFeaturePackageHooks,
  enabledLinuxFeatureStageHooks,
  loadEnabledLinuxFeatures,
  loadLinuxFeaturePatchDescriptors,
} = require(path.join(repoRoot, "scripts", "lib", "linux-features.js"));

function fail(message) {
  throw new Error(message);
}

function requiredClosure(feature, featureMap, enabled = new Set()) {
  if (enabled.has(feature.id)) return enabled;
  enabled.add(feature.id);
  for (const requiredId of feature.manifest.requires) {
    const required = featureMap.get(requiredId);
    if (required == null) {
      fail(`Linux feature '${feature.id}' requires missing feature '${requiredId}'`);
    }
    requiredClosure(required, featureMap, enabled);
  }
  return enabled;
}

function assertEntrypointsExist(feature) {
  for (const [name, entry] of Object.entries(feature.manifest.entrypoints ?? {})) {
    const entries = Array.isArray(entry) ? entry : [entry];
    for (const value of entries) {
      if (typeof value !== "string" || value.length === 0) {
        fail(`Linux feature '${feature.id}' has an invalid ${name} entrypoint`);
      }
      const resolved = path.resolve(feature.dir, value);
      const relative = path.relative(feature.dir, resolved);
      if (relative.startsWith("..") || path.isAbsolute(relative)) {
        fail(`Linux feature '${feature.id}' ${name} entrypoint escapes its directory`);
      }
      if (!fs.existsSync(resolved)) {
        fail(`Linux feature '${feature.id}' ${name} entrypoint is missing: ${resolved}`);
      }
    }
  }
}

function validateFeatureContract(feature, featureMap, tempRoot) {
  const testPath = path.join(feature.dir, "test.js");
  if (!fs.statSync(feature.readmePath).isFile() || fs.readFileSync(feature.readmePath, "utf8").trim() === "") {
    fail(`Linux feature '${feature.id}' must have a non-empty README.md`);
  }
  if (!fs.existsSync(testPath) || !fs.statSync(testPath).isFile()) {
    fail(`Linux feature '${feature.id}' must include test.js`);
  }
  if (path.basename(feature.dir) !== feature.id) {
    fail(`Linux feature '${feature.id}' directory must have the same name as its id`);
  }

  assertEntrypointsExist(feature);
  const enabled = [...requiredClosure(feature, featureMap)].sort();
  const configPath = path.join(tempRoot, `${feature.id}.json`);
  fs.writeFileSync(configPath, `${JSON.stringify({ enabled }, null, 2)}\n`);
  const options = { featuresRoot, featuresConfigPath: configPath };

  loadEnabledLinuxFeatures(options);
  enabledLinuxFeatureInstallPlan(options);
  enabledLinuxFeatureStageHooks(options);
  loadLinuxFeaturePatchDescriptors(options);
  for (const packageFormat of ["deb", "rpm", "pacman", "gentoo", "apk", "appimage"]) {
    enabledLinuxFeaturePackageHooks({ ...options, packageFormat });
  }

  return testPath;
}

function runFeatureTest(feature, testPath) {
  process.stdout.write(`\n=== Linux feature: ${feature.id} ===\n`);
  const result = spawnSync(process.execPath, ["--test", testPath], {
    cwd: repoRoot,
    env: process.env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    fail(`Linux feature '${feature.id}' test suite failed with status ${result.status}`);
  }
}

function main() {
  const features = discoverLinuxFeatureManifests({ featuresRoot });
  if (features.length === 0) fail("No repository Linux features were discovered");
  const featureMap = new Map(features.map((feature) => [feature.id, feature]));
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codex-linux-features-"));

  try {
    const suites = features.map((feature) => ({
      feature,
      testPath: validateFeatureContract(feature, featureMap, tempRoot),
    }));
    for (const { feature, testPath } of suites) runFeatureTest(feature, testPath);
    process.stdout.write(`\nValidated ${features.length} Linux feature contracts and test suites.\n`);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
}

try {
  main();
} catch (error) {
  process.stderr.write(`ERROR: ${error.message}\n`);
  process.exitCode = 1;
}
