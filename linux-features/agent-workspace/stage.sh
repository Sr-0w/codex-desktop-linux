#!/usr/bin/env bash
set -Eeuo pipefail

feature_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
skill_source="$feature_dir/skills/agent-workspace-linux/SKILL.md"

if [ ! -f "$skill_source" ]; then
    echo "Agent Workspaces skill source not found at $skill_source" >&2
    exit 1
fi

codex_home="${CODEX_HOME:-}"
if [ -z "$codex_home" ]; then
    if [ -z "${HOME:-}" ]; then
        echo "CODEX_HOME is not set and HOME is unavailable; cannot install Agent Workspaces skill" >&2
        exit 1
    fi
    codex_home="$HOME/.codex"
fi

target_dir="$codex_home/skills/agent-workspace-linux"
target_skill="$target_dir/SKILL.md"

mkdir -p "$target_dir"
install -m 0644 "$skill_source" "$target_skill"

echo "Installed Agent Workspaces skill to $target_skill" >&2
