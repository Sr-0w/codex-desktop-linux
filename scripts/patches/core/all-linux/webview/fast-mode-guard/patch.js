"use strict";

const { applyLinuxFastModeModelGuardPatch } = require("../../../../webview-assets.js");

module.exports = [
  {
    id: "linux-fast-mode-model-guard",
    phase: "webview-asset",
    order: 1040,
    ciPolicy: "required-upstream",
    // Vite chunk names and split points are not an API. The patch itself is
    // content-gated, so scan JavaScript assets and let unsafe service-tier code
    // identify the owning bundle.
    pattern: /\.(?:c|m)?js$/,
    missingDescription: "JavaScript webview assets for fast-mode validation",
    skipDescription: "fast-mode model guard patch",
    apply: applyLinuxFastModeModelGuardPatch,
  },
];
