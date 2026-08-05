#!/usr/bin/env bash

# This hook is intentionally fail-soft. Missing KDE tools must never block Codex.
set -u

is_kwin_session() {
    local desktop="${XDG_CURRENT_DESKTOP:-}:${DESKTOP_SESSION:-}"
    desktop="${desktop,,}"
    case "$desktop" in
        *kde*|*plasma*) return 0 ;;
    esac
    command -v qdbus6 >/dev/null 2>&1 || return 1
    qdbus6 org.kde.KWin /KWin org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1
}

read_config() {
    kreadconfig6 --file kwinrulesrc --group "$1" --key "$2" 2>/dev/null || true
}

write_config() {
    kwriteconfig6 --file kwinrulesrc --group "$1" --key "$2" "$3" >/dev/null 2>&1 || true
}

write_bool_config() {
    kwriteconfig6 --file kwinrulesrc --group "$1" --key "$2" --type bool "$3" >/dev/null 2>&1 || true
}

delete_config() {
    kwriteconfig6 --file kwinrulesrc --group "$1" --key "$2" --delete "" >/dev/null 2>&1 || true
}

load_pet_overlay_script() {
    command -v qdbus6 >/dev/null 2>&1 || return 0

    local runtime_root="${XDG_RUNTIME_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}}"
    local script_dir="$runtime_root/codex-desktop-linux/kwin"
    local script_path="$script_dir/pet-overlay-$safe_app_id.js"
    local script_id="codex-pet-overlay-live-$safe_app_id"

    mkdir -p "$script_dir" >/dev/null 2>&1 || return 0
    {
        printf 'var codexAppId = "%s";\n' "$safe_app_id"
        cat <<'KWIN_SCRIPT'
function codexPetMatches(window) {
    var caption = String(window.caption || "");
    var resourceClass = String(window.resourceClass || "").toLowerCase();
    if (resourceClass !== codexAppId.toLowerCase()) {
        return false;
    }
    if (caption === "Codex Pet Overlay") {
        return true;
    }
    var geometry = window.frameGeometry;
    var legacyPetSize = geometry && Number(geometry.width) > 0 && Number(geometry.height) > 0 &&
        Number(geometry.width) <= 512 && Number(geometry.height) <= 512;
    return caption === "Codex" && window.normalWindow && legacyPetSize;
}

function codexKeepPetAbove(window) {
    if (!codexPetMatches(window)) {
        return;
    }
    window.keepAbove = true;
    window.noBorder = true;
    window.skipTaskbar = true;
    window.skipPager = true;
    window.skipSwitcher = true;
}

function codexConnectWindowSignal(signal, callback) {
    try {
        if (signal) {
            signal.connect(callback);
        }
    } catch (_) {
    }
}

function codexWatchWindow(window) {
    var refresh = function () {
        codexKeepPetAbove(window);
    };
    refresh();
    codexConnectWindowSignal(window.captionChanged, refresh);
    codexConnectWindowSignal(window.frameGeometryChanged, refresh);
    codexConnectWindowSignal(window.skipTaskbarChanged, refresh);
    codexConnectWindowSignal(window.noBorderChanged, refresh);
}

workspace.windowAdded.connect(codexWatchWindow);
var codexWindows = workspace.windowList();
for (var codexWindowIndex = 0; codexWindowIndex < codexWindows.length; codexWindowIndex++) {
    codexWatchWindow(codexWindows[codexWindowIndex]);
}
KWIN_SCRIPT
    } > "$script_path" || return 0

    qdbus6 org.kde.KWin /Scripting \
        org.kde.kwin.Scripting.unloadScript "$script_id" >/dev/null 2>&1 || true
    qdbus6 org.kde.KWin /Scripting \
        org.kde.kwin.Scripting.loadScript "$script_path" "$script_id" >/dev/null 2>&1 || return 0
    qdbus6 org.kde.KWin /Scripting \
        org.kde.kwin.Scripting.start >/dev/null 2>&1 || true
}

append_rule_id() {
    local existing_rules="$1"
    local wanted_rule="$2"
    local item
    local joined=""

    IFS=',' read -r -a rule_items <<< "$existing_rules"
    for item in "${rule_items[@]}"; do
        [ -n "$item" ] || continue
        if [ "$item" = "$wanted_rule" ]; then
            RULE_ALREADY_PRESENT=1
        fi
        if [ -z "$joined" ]; then
            joined="$item"
        else
            joined="$joined,$item"
        fi
    done
    if [ "$RULE_ALREADY_PRESENT" -eq 0 ]; then
        if [ -n "$joined" ]; then
            joined="$joined,$wanted_rule"
        else
            joined="$wanted_rule"
        fi
    fi
    printf '%s\n' "$joined"
}

rule_count() {
    local rules="$1"
    local item
    local count=0
    IFS=',' read -r -a rule_items <<< "$rules"
    for item in "${rule_items[@]}"; do
        [ -n "$item" ] || continue
        count=$((count + 1))
    done
    printf '%s\n' "$count"
}

is_kwin_session || exit 0
command -v kwriteconfig6 >/dev/null 2>&1 || exit 0
command -v kreadconfig6 >/dev/null 2>&1 || exit 0

app_id="${CODEX_LINUX_APP_ID:-codex-desktop}"
safe_app_id="$(printf '%s' "$app_id" | tr -c 'A-Za-z0-9_-' '-')"
rule_group="codex-pet-overlay-$safe_app_id"
rules="$(read_config General rules)"
RULE_ALREADY_PRESENT=0
rules="$(append_rule_id "$rules" "$rule_group")"

write_config General rules "$rules"
write_config General count "$(rule_count "$rules")"

write_config "$rule_group" Description "Codex Desktop pet overlay"
write_config "$rule_group" wmclass "$app_id"
write_config "$rule_group" wmclassmatch 1
write_bool_config "$rule_group" wmclasscomplete false
write_config "$rule_group" title "Codex Pet Overlay"
write_config "$rule_group" titlematch 1
write_config "$rule_group" types 1
write_bool_config "$rule_group" noborder true
write_config "$rule_group" noborderrule 2
write_bool_config "$rule_group" above true
write_config "$rule_group" aboverule 2
write_bool_config "$rule_group" skiptaskbar true
write_config "$rule_group" skiptaskbarrule 2
write_bool_config "$rule_group" skippager true
write_config "$rule_group" skippagerrule 2
write_bool_config "$rule_group" skipswitcher true
write_config "$rule_group" skipswitcherrule 2

# Remove constraints from an older broad rule that matched every Codex window.
legacy_description="$(read_config "$app_id" Description)"
if [ "$legacy_description" = "Codex Desktop KDE integration" ]; then
    for key in \
        noborder noborderrule \
        skiptaskbar skiptaskbarrule \
        skippager skippagerrule \
        skipswitcher skipswitcherrule
    do
        delete_config "$app_id" "$key"
    done
fi

qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure >/dev/null 2>&1 || true
load_pet_overlay_script
exit 0
