#!/bin/bash
set -Eeuo pipefail

APP_DIR="${1:-codex-app}"
EXPECTED_ARCH="${2:-$(uname -m)}"

case "$EXPECTED_ARCH" in
    x86_64|amd64|x64)
        EXPECTED_ARCH="x86_64"
        EXPECTED_MACHINE=62
        NODE_MODULE_ARCH="x64"
        CHROME_HOST_ARCH="x64"
        ;;
    aarch64|arm64)
        EXPECTED_ARCH="aarch64"
        EXPECTED_MACHINE=183
        NODE_MODULE_ARCH="arm64"
        CHROME_HOST_ARCH="arm64"
        ;;
    *)
        echo "Unsupported expected architecture: $EXPECTED_ARCH" >&2
        exit 2
        ;;
esac

[ -d "$APP_DIR" ] || {
    echo "App directory not found: $APP_DIR" >&2
    exit 2
}

checked=0

check_elf_arch() {
    local path="$1"
    local label="$2"
    local required="${3:-1}"

    if [ ! -f "$path" ]; then
        if [ "$required" = "1" ]; then
            echo "Missing $label: $path" >&2
            return 1
        fi
        return 0
    fi

    if ! python3 - "$path" "$EXPECTED_MACHINE" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
expected_machine = int(sys.argv[2])
header = path.read_bytes()[:20]
if len(header) < 20 or header[:4] != b"\x7fELF":
    raise SystemExit(1)
if header[4] != 2 or header[5] != 1:
    raise SystemExit(1)
machine = int.from_bytes(header[18:20], "little")
raise SystemExit(0 if machine == expected_machine else 1)
PY
    then
        echo "$label does not target $EXPECTED_ARCH: $path" >&2
        return 1
    fi

    checked=$((checked + 1))
}

check_matching_files() {
    local root="$1"
    local pattern="$2"
    local label="$3"
    local required="${4:-0}"
    local found=0
    local path

    [ -d "$root" ] || {
        if [ "$required" = "1" ]; then
            echo "Missing $label directory: $root" >&2
            return 1
        fi
        return 0
    }

    while IFS= read -r -d '' path; do
        found=1
        check_elf_arch "$path" "$label"
    done < <(find "$root" -type f -name "$pattern" -print0)

    if [ "$required" = "1" ] && [ "$found" = "0" ]; then
        echo "No $label files found under $root" >&2
        return 1
    fi
}

check_elf_arch "$APP_DIR/electron" "Electron"
check_elf_arch "$APP_DIR/chrome_crashpad_handler" "Electron crash handler"
check_elf_arch "$APP_DIR/chrome-sandbox" "Electron sandbox"
check_elf_arch "$APP_DIR/resources/node-runtime/bin/node" "managed Node.js"
check_elf_arch "$APP_DIR/resources/node_repl" "Browser Use node_repl" 0

check_matching_files \
    "$APP_DIR/resources/app.asar.unpacked/node_modules/better-sqlite3/build/Release" \
    '*.node' \
    "better-sqlite3 native module" \
    1
check_matching_files \
    "$APP_DIR/resources/app.asar.unpacked/node_modules/node-pty/build/Release" \
    '*.node' \
    "node-pty native module" \
    1

for path in \
    "$APP_DIR/resources/plugins/openai-bundled/plugins/computer-use/bin/codex-computer-use-linux" \
    "$APP_DIR/resources/plugins/openai-bundled/plugins/computer-use/bin/codex-computer-use-cosmic" \
    "$APP_DIR/resources/plugins/openai-bundled/plugins/chrome/extension-host/linux/$CHROME_HOST_ARCH/extension-host"
do
    check_elf_arch "$path" "bundled Linux helper" 0
done

for plugin in browser chrome; do
    classic_level_root="$APP_DIR/resources/plugins/openai-bundled/plugins/$plugin/scripts/node_modules/classic-level/prebuilds/linux-$NODE_MODULE_ARCH"
    if [ -d "$classic_level_root" ]; then
        check_matching_files "$classic_level_root" '*.node' "$plugin classic-level native module" 1
    fi
done

printf 'Validated %s native runtime files for %s.\n' "$checked" "$EXPECTED_ARCH"
