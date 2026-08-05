"use strict";

const {
  applySubagentNicknameMetadataPatch,
} = require("../../../../webview-assets.js");

module.exports = [
  {
    id: "subagent-nickname-metadata-shape",
    phase: "webview-asset",
    order: 1050,
    ciPolicy: "required-upstream",
    // Vite chunk names and split points are not stable. `thread_spawn` and the
    // metadata shapes gate the patch itself, so discover the owning bundle by
    // content instead of maintaining a filename allowlist.
    pattern: /\.(?:c|m)?js$/,
    missingDescription: "JavaScript webview assets for subagent metadata validation",
    skipDescription: "subagent nickname metadata shape patch",
    apply: applySubagentNicknameMetadataPatch,
  },
];
