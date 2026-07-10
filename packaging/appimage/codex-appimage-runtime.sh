#!/bin/bash

CODEX_APPIMAGE_CURRENT_VERSION="__VERSION__"
CODEX_APPIMAGE_RELEASE_API_URL="${CODEX_APPIMAGE_RELEASE_API_URL:-https://api.github.com/repos/Sr-0w/codex-desktop-linux/releases/latest}"
CODEX_APPIMAGE_RELEASES_URL="${CODEX_APPIMAGE_RELEASES_URL:-https://github.com/Sr-0w/codex-desktop-linux/releases/latest}"

codex_packaged_runtime_export_env() {
    export CHROME_DESKTOP="__PACKAGE_NAME__.desktop"

    if [ -n "${APPDIR:-}" ] && [ -f "$APPDIR/__PACKAGE_NAME__.desktop" ]; then
        export BAMF_DESKTOP_FILE_HINT="$APPDIR/__PACKAGE_NAME__.desktop"
    else
        export BAMF_DESKTOP_FILE_HINT="__PACKAGE_NAME__.desktop"
    fi
}

codex_packaged_runtime_prelaunch() {
    codex_appimage_update_check_background >/dev/null 2>&1 &
}

codex_appimage_update_check_background() {
    case "${CODEX_APPIMAGE_UPDATE_CHECK:-1}" in
        0|false|False|FALSE|no|No|NO|off|Off|OFF)
            return 0
            ;;
    esac

    command -v node >/dev/null 2>&1 || return 0

    local state_root
    local update_state_dir
    local check_file
    local notified_file
    local now
    local last_check
    local interval

    state_root="${CODEX_LINUX_APP_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/${CODEX_LINUX_APP_ID:-codex-desktop}}"
    update_state_dir="$state_root/appimage-updates"
    check_file="$update_state_dir/last-check"
    notified_file="$update_state_dir/notified-tag"
    interval="${CODEX_APPIMAGE_UPDATE_CHECK_INTERVAL_SECONDS:-21600}"
    now="$(date +%s 2>/dev/null || printf '0')"
    last_check="0"

    if [ "${CODEX_APPIMAGE_FORCE_UPDATE_CHECK:-0}" != "1" ] && [ -r "$check_file" ]; then
        last_check="$(sed -n '1p' "$check_file" 2>/dev/null || printf '0')"
        case "$last_check" in
            ''|*[!0-9]*) last_check="0" ;;
        esac
        case "$interval" in
            ''|*[!0-9]*) interval="21600" ;;
        esac
        if [ "$now" -gt 0 ] && [ "$((now - last_check))" -lt "$interval" ]; then
            return 0
        fi
    fi

    mkdir -p "$update_state_dir" || return 0
    printf '%s\n' "$now" > "$check_file" 2>/dev/null || true

    local release_info
    if ! release_info="$(codex_appimage_fetch_latest_release)"; then
        return 0
    fi

    local latest_tag
    local release_url
    local appimage_url
    latest_tag="$(printf '%s\n' "$release_info" | sed -n '1p')"
    release_url="$(printf '%s\n' "$release_info" | sed -n '2p')"
    appimage_url="$(printf '%s\n' "$release_info" | sed -n '3p')"
    [ -n "$latest_tag" ] || return 0
    [ -n "$release_url" ] || release_url="$CODEX_APPIMAGE_RELEASES_URL"

    local current_key
    local latest_key
    current_key="$(codex_appimage_release_key "$CODEX_APPIMAGE_CURRENT_VERSION")"
    latest_key="$(codex_appimage_release_key "$latest_tag")"
    [ -n "$current_key" ] || return 0
    [ -n "$latest_key" ] || return 0

    if [[ "$latest_key" == "$current_key" || "$latest_key" < "$current_key" ]]; then
        return 0
    fi

    if [ -r "$notified_file" ] && [ "$(sed -n '1p' "$notified_file" 2>/dev/null || true)" = "$latest_tag" ]; then
        return 0
    fi

    codex_appimage_prompt_release_update "$latest_tag" "$release_url" "$appimage_url"
    printf '%s\n' "$latest_tag" > "$notified_file" 2>/dev/null || true
}

codex_appimage_release_key() {
    printf '%s\n' "${1:-}" |
        sed -n 's/^v\{0,1\}\([0-9][0-9][0-9][0-9]\(\.[0-9][0-9]\)\{2\}\.[0-9][0-9][0-9][0-9][0-9][0-9]\).*/\1/p'
}

codex_appimage_fetch_latest_release() {
    CODEX_APPIMAGE_RELEASE_API_URL="$CODEX_APPIMAGE_RELEASE_API_URL" node <<'NODE'
const https = require("node:https");

const apiUrl = process.env.CODEX_APPIMAGE_RELEASE_API_URL;
if (!apiUrl) {
  process.exit(2);
}

const request = https.get(apiUrl, {
  headers: {
    "Accept": "application/vnd.github+json",
    "User-Agent": "codex-desktop-linux-appimage-update-check",
  },
  timeout: 15000,
}, (response) => {
  if (response.statusCode < 200 || response.statusCode >= 300) {
    response.resume();
    process.exit(3);
  }

  let body = "";
  response.setEncoding("utf8");
  response.on("data", (chunk) => {
    body += chunk;
    if (body.length > 1024 * 1024) {
      request.destroy(new Error("release response too large"));
    }
  });
  response.on("end", () => {
    const release = JSON.parse(body);
    const assets = Array.isArray(release.assets) ? release.assets : [];
    const appImage = assets.find((asset) =>
      typeof asset?.name === "string" &&
      asset.name.endsWith(".AppImage") &&
      typeof asset?.browser_download_url === "string"
    );
    console.log(release.tag_name || "");
    console.log(release.html_url || "");
    console.log(appImage?.browser_download_url || "");
  });
});

request.on("timeout", () => request.destroy(new Error("release check timed out")));
request.on("error", () => process.exit(4));
NODE
}

codex_appimage_prompt_release_update() {
    local latest_tag="$1"
    local release_url="$2"
    local appimage_url="$3"
    local target_url="${appimage_url:-$release_url}"
    local title="Codex Desktop update available"
    local text="Codex Desktop Linux $latest_tag is available. Download and install it now? The update will be used the next time you launch Codex."

    case "${CODEX_APPIMAGE_UPDATE_PROMPT:-1}" in
        0|false|False|FALSE|no|No|NO|off|Off|OFF)
            codex_appimage_notify_release_update "$latest_tag" "$release_url" "$appimage_url"
            return 0
            ;;
    esac

    if [ "${CODEX_APPIMAGE_UPDATE_ASSUME_YES:-0}" = "1" ]; then
        codex_appimage_install_release_update "$latest_tag" "$appimage_url" "$release_url"
        return 0
    fi

    if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && command -v kdialog >/dev/null 2>&1; then
        if kdialog --title "$title" --yesno "$text"; then
            if [ -n "$appimage_url" ]; then
                codex_appimage_install_release_update "$latest_tag" "$appimage_url" "$release_url"
            else
                codex_appimage_open_url "$target_url"
            fi
        fi
        return 0
    fi

    if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && command -v zenity >/dev/null 2>&1; then
        if zenity --question --title="$title" --text="$text"; then
            if [ -n "$appimage_url" ]; then
                codex_appimage_install_release_update "$latest_tag" "$appimage_url" "$release_url"
            else
                codex_appimage_open_url "$target_url"
            fi
        fi
        return 0
    fi

    codex_appimage_notify_release_update "$latest_tag" "$release_url" "$appimage_url"
}

codex_appimage_install_release_update() {
    local latest_tag="$1"
    local appimage_url="$2"
    local release_url="$3"
    local current_path="${CODEX_APPIMAGE_PATH:-${APPIMAGE:-}}"

    if [ -z "$appimage_url" ] || [ -z "$current_path" ] || [ ! -f "$current_path" ] || ! command -v curl >/dev/null 2>&1; then
        codex_appimage_open_url "${appimage_url:-$release_url}"
        return 0
    fi

    local install_dir
    local base_name
    local tmp_dir
    local download_path
    install_dir="$(dirname "$current_path")"
    base_name="$(basename "$current_path")"
    tmp_dir="$(mktemp -d "$install_dir/.codex-appimage-update.XXXXXX")" || return 0
    download_path="$tmp_dir/$base_name"

    codex_appimage_notify_message "Codex Desktop update" "Downloading $latest_tag..."

    if ! curl -fL --retry 3 \
        -H "User-Agent: codex-desktop-linux-appimage-updater" \
        -o "$download_path" \
        "$appimage_url"; then
        rm -rf "$tmp_dir"
        codex_appimage_notify_message "Codex Desktop update failed" "Could not download $latest_tag. Open GitHub Releases to update manually."
        codex_appimage_open_url "$release_url"
        return 0
    fi

    chmod 0755 "$download_path" 2>/dev/null || true
    if ! codex_appimage_verify_download "$download_path" "$appimage_url" "$latest_tag"; then
        rm -rf "$tmp_dir"
        codex_appimage_notify_message "Codex Desktop update failed" "Downloaded AppImage could not be verified."
        return 0
    fi

    local backup_path
    backup_path="$current_path.before-update-$(date +%Y%m%d%H%M%S)"
    if ! mv "$current_path" "$backup_path"; then
        rm -rf "$tmp_dir"
        codex_appimage_notify_message "Codex Desktop update failed" "Could not replace the current AppImage."
        codex_appimage_open_url "$release_url"
        return 0
    fi

    if ! mv "$download_path" "$current_path"; then
        mv "$backup_path" "$current_path" 2>/dev/null || true
        rm -rf "$tmp_dir"
        codex_appimage_notify_message "Codex Desktop update failed" "Could not install the downloaded AppImage."
        return 0
    fi

    chmod 0755 "$current_path" 2>/dev/null || true
    rm -rf "$tmp_dir"
    codex_appimage_notify_message "Codex Desktop update installed" "Restart Codex to use $latest_tag. Previous AppImage saved next to the app."
}

codex_appimage_verify_download() {
    local download_path="$1"
    local appimage_url="$2"
    local latest_tag="$3"
    local checksum_url="${appimage_url}.sha256"
    local checksum_file
    local expected
    local actual
    local extract_dir
    local downloaded_version

    if command -v sha256sum >/dev/null 2>&1; then
        checksum_file="$(mktemp)"
        if curl -fL --retry 3 \
            -H "User-Agent: codex-desktop-linux-appimage-updater" \
            -o "$checksum_file" \
            "$checksum_url"; then
            expected="$(awk '{print $1; exit}' "$checksum_file")"
            actual="$(sha256sum "$download_path" | awk '{print $1; exit}')"
            rm -f "$checksum_file"
            [ -n "$expected" ] || return 1
            [ "$expected" = "$actual" ] || return 1
        else
            rm -f "$checksum_file"
        fi
    fi

    "$download_path" --appimage-help >/dev/null 2>&1 || return 1

    extract_dir="$(mktemp -d)"
    (
        cd "$extract_dir" &&
            "$download_path" --appimage-extract codex-desktop.desktop >/dev/null 2>&1
    ) || {
        rm -rf "$extract_dir"
        return 1
    }
    downloaded_version="$(sed -n 's/^X-AppImage-Version=//p' "$extract_dir/squashfs-root/codex-desktop.desktop" 2>/dev/null | head -n 1)"
    rm -rf "$extract_dir"

    [ -n "$downloaded_version" ] || return 1
    [ "$(codex_appimage_release_key "$downloaded_version")" = "$(codex_appimage_release_key "$latest_tag")" ] || return 1
}

codex_appimage_notify_release_update() {
    local latest_tag="$1"
    local release_url="$2"
    local appimage_url="${3:-}"

    if [ -n "$appimage_url" ]; then
        codex_appimage_notify_message "Codex Desktop update available" "Run Check for Updates to install $latest_tag, or download it from $release_url"
    else
        codex_appimage_notify_message "Codex Desktop update available" "Download $latest_tag from $release_url"
    fi
}

codex_appimage_notify_message() {
    local title="$1"
    local body="$2"
    local icon="${APP_NOTIFICATION_ICON_BUNDLE:-__PACKAGE_NAME__}"

    if command -v notify-send >/dev/null 2>&1; then
        notify-send \
            -a "${CODEX_LINUX_APP_DISPLAY_NAME:-Codex Desktop}" \
            -i "$icon" \
            -h "string:desktop-entry:${CODEX_LINUX_APP_ID:-__PACKAGE_NAME__}" \
            "$title" \
            "$body"
    fi
}

codex_appimage_open_url() {
    local url="$1"

    if command -v xdg-open >/dev/null 2>&1; then
        xdg-open "$url" >/dev/null 2>&1 &
    elif command -v kde-open5 >/dev/null 2>&1; then
        kde-open5 "$url" >/dev/null 2>&1 &
    elif command -v gio >/dev/null 2>&1; then
        gio open "$url" >/dev/null 2>&1 &
    fi
}
