const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const { parseDebianStanzas, validateVersion } = require("./validate-upstream-release-version.js");

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "codex-upstream-version-"));
  const packages = path.join(root, "Packages");
  fs.writeFileSync(packages, [
    "Package: chatgpt",
    "Version: 26.810.52044",
    "Architecture: amd64",
    "Filename: pool/chatgpt.deb",
    "",
  ].join("\n"));
  return { root, packages };
}

test("parses Debian control stanzas", () => {
  assert.deepEqual(parseDebianStanzas("Package: chatgpt\nVersion: 1\n\nPackage: other\nVersion: 2\n"), [
    { Package: "chatgpt", Version: "1" },
    { Package: "other", Version: "2" },
  ]);
});

test("accepts a DMG version matching the official Linux repository", (t) => {
  const { root, packages } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.equal(validateVersion({ version: "26.810.52044", packages, architecture: "amd64", packageName: "chatgpt" }), "26.810.52044");
});

test("rejects a stale CDN edge", (t) => {
  const { root, packages } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.throws(
    () => validateVersion({ version: "26.810.50856", packages, architecture: "amd64", packageName: "chatgpt" }),
    /Stale DMG edge detected.*26\.810\.50856.*26\.810\.52044/,
  );
});

test("reads the DMG version from build-info.json", (t) => {
  const { root, packages } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const buildInfo = path.join(root, "build-info.json");
  fs.writeFileSync(buildInfo, JSON.stringify({ upstreamDmg: { appVersion: "26.810.52044" } }));
  assert.equal(validateVersion({ buildInfo, packages, architecture: "amd64", packageName: "chatgpt" }), "26.810.52044");
});
