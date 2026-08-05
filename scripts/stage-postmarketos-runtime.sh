#!/bin/bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
. "$REPO_DIR/scripts/lib/package-common.sh"

APP_ROOT="${1:-${APP_DIR_OVERRIDE:-$REPO_DIR/codex-app}}"
# Used by helpers sourced from package-common.sh.
# shellcheck disable=SC2034
APP_DIR="$APP_ROOT"
INSTALL_ROOT="${2:-/opt/${PACKAGE_NAME:-codex-desktop}}"
RUNTIME_ROOT="$APP_ROOT/.codex-linux/glibc-runtime"
RUNTIME_LIB="$RUNTIME_ROOT/lib"
RUNTIME_DRI="$RUNTIME_ROOT/dri"
RUNTIME_LICENSES="$RUNTIME_ROOT/licenses"
TARGET_RUNTIME_LIB="$INSTALL_ROOT/.codex-linux/glibc-runtime/lib"
MANIFEST="$RUNTIME_ROOT/manifest.sha256"

declare -A COPIED_LIBRARIES=()
declare -A COPIED_LICENSES=()

is_elf_file() {
    local path="$1"
    [ -f "$path" ] || return 1
    [ "$(od -An -tx1 -N4 "$path" 2>/dev/null | tr -d ' \n')" = "7f454c46" ]
}

elf_machine() {
    readelf -h "$1" | awk -F: '$1 ~ /Machine/ {gsub(/^[[:space:]]+/, "", $2); print $2; exit}'
}

loader_name_for_machine() {
    case "$1" in
        *AArch64*) printf '%s\n' "ld-linux-aarch64.so.1" ;;
        *X86-64*) printf '%s\n' "ld-linux-x86-64.so.2" ;;
        *) error "Unsupported postmarketOS ELF machine: $1" ;;
    esac
}

loader_source_for_machine() {
    local machine="$1"
    local candidate

    case "$machine" in
        *AArch64*)
            for candidate in /lib/ld-linux-aarch64.so.1 /lib64/ld-linux-aarch64.so.1 \
                /lib/aarch64-linux-gnu/ld-linux-aarch64.so.1; do
                [ -f "$candidate" ] && { printf '%s\n' "$candidate"; return 0; }
            done
            ;;
        *X86-64*)
            for candidate in /lib64/ld-linux-x86-64.so.2 \
                /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2; do
                [ -f "$candidate" ] && { printf '%s\n' "$candidate"; return 0; }
            done
            ;;
    esac
    error "Could not find the host glibc loader for $machine"
}

private_rpath() {
    local current_rpath="$1"
    local entry
    local result=""
    local -a entries=()

    IFS=: read -r -a entries <<< "$current_rpath"
    for entry in "${entries[@]}"; do
        [ -n "$entry" ] || continue
        # Keep the literal loader token out of the inherited entries.
        # shellcheck disable=SC2016
        [ "$entry" != '$ORIGIN' ] || continue
        case "$entry" in
            */.codex-linux/glibc-runtime/lib) continue ;;
        esac
        result="${result:+$result:}$entry"
    done

    printf '%s\n' "${result:+$result:}\$ORIGIN:$TARGET_RUNTIME_LIB"
}

copy_runtime_library() {
    local source="$1"
    local name
    local target

    [ -f "$source" ] || return 0
    name="$(basename "$source")"
    target="$RUNTIME_LIB/$name"

    if [ -n "${COPIED_LIBRARIES[$name]:-}" ]; then
        if ! cmp -s "$source" "$target"; then
            error "Conflicting private glibc libraries named $name: ${COPIED_LIBRARIES[$name]} and $source"
        fi
        return 0
    fi

    cp -L --preserve=mode,timestamps "$source" "$target"
    COPIED_LIBRARIES[$name]="$source"
    copy_runtime_license_metadata "$source"
}

copy_runtime_license_metadata() {
    local source="$1"
    local canonical_source
    local package
    local copyright

    command -v dpkg-query >/dev/null 2>&1 || return 0
    canonical_source="$(readlink -f "$source" 2>/dev/null || printf '%s' "$source")"
    package="$(dpkg-query -S "$canonical_source" "$source" 2>/dev/null \
        | awk -F: 'NR == 1 {print $1}')"
    [ -n "$package" ] || return 0
    [ -z "${COPIED_LICENSES[$package]:-}" ] || return 0
    copyright="/usr/share/doc/$package/copyright"
    [ -f "$copyright" ] || return 0

    mkdir -p "$RUNTIME_LICENSES/packages"
    cp -L "$copyright" "$RUNTIME_LICENSES/packages/$package.copyright"
    COPIED_LICENSES[$package]="$copyright"
}

collect_elf_dependencies() {
    local elf="$1"
    local dependency

    while IFS= read -r dependency; do
        case "$dependency" in
            /*)
                [ "$dependency" != "$elf" ] || continue
                case "$dependency" in
                    "$APP_ROOT"/*) continue ;;
                esac
                copy_runtime_library "$dependency"
                ;;
        esac
    done < <(lddtree -l "$elf")
}

stage_dri_drivers() {
    local machine="$1"
    local enabled="${POSTMARKETOS_STAGE_DRI:-auto}"
    local required_v3d="${POSTMARKETOS_REQUIRE_V3D:-auto}"
    local source_dir
    local driver_name
    local source
    local target
    local current_rpath
    local found=0
    local found_v3d=0
    local -a source_dirs=()
    local -a driver_names=(
        v3d_dri.so
        vc4_dri.so
        panfrost_dri.so
        msm_dri.so
        virtio_gpu_dri.so
        kms_swrast_dri.so
        swrast_dri.so
    )

    case "$enabled" in
        0|false|no|off) return 0 ;;
        auto)
            case "$machine" in
                *AArch64*) ;;
                *) return 0 ;;
            esac
            ;;
        1|true|yes|on) ;;
        *) error "POSTMARKETOS_STAGE_DRI must be auto, 1, or 0" ;;
    esac
    if [ "$required_v3d" = "auto" ]; then
        case "$machine" in
            *AArch64*) required_v3d=1 ;;
            *) required_v3d=0 ;;
        esac
    fi

    if [ -n "${POSTMARKETOS_DRI_SOURCE_DIRS:-}" ]; then
        IFS=: read -r -a source_dirs <<< "$POSTMARKETOS_DRI_SOURCE_DIRS"
    else
        while IFS= read -r -d '' source_dir; do
            source_dirs+=("$source_dir")
        done < <(find /usr/lib /usr/lib64 /lib /lib64 -type d -name dri -print0 2>/dev/null)
    fi

    mkdir -p "$RUNTIME_DRI"
    for driver_name in "${driver_names[@]}"; do
        source=""
        for source_dir in "${source_dirs[@]}"; do
            if [ -f "$source_dir/$driver_name" ]; then
                source="$source_dir/$driver_name"
                break
            fi
        done
        [ -n "$source" ] || continue
        [ "$(elf_machine "$source")" = "$machine" ] || continue

        target="$RUNTIME_DRI/$driver_name"
        cp -L --preserve=mode,timestamps "$source" "$target"
        copy_runtime_license_metadata "$source"
        collect_elf_dependencies "$source"
        current_rpath="$(patchelf --print-rpath "$source" 2>/dev/null || true)"
        patchelf --force-rpath --set-rpath "$(private_rpath "$current_rpath")" "$target"
        found=$((found + 1))
        [ "$driver_name" != "v3d_dri.so" ] || found_v3d=1
    done

    if [ "$required_v3d" = "1" ] && [ "$found_v3d" -ne 1 ]; then
        error "v3d_dri.so is required for the Raspberry Pi 4 postmarketOS package"
    fi
    if [ "$found" -eq 0 ]; then
        rmdir "$RUNTIME_DRI"
        warn "No private Mesa DRI drivers were staged"
    else
        info "Staged $found private Mesa DRI driver(s)"
    fi
}

copy_optional_glibc_service_library() {
    local name="$1"
    local candidate

    candidate="$(ldconfig -p 2>/dev/null | awk -v name="$name" '$1 == name && value == "" {value = $NF} END {print value}')"
    [ -n "$candidate" ] || return 0
    copy_runtime_library "$candidate"
    collect_elf_dependencies "$candidate"
}

patch_app_elf() {
    local elf="$1"
    local loader_path="$2"
    local current_interpreter
    local current_rpath
    local rpath

    current_rpath="$(patchelf --print-rpath "$elf" 2>/dev/null || true)"
    rpath="$(private_rpath "$current_rpath")"

    if current_interpreter="$(patchelf --print-interpreter "$elf" 2>/dev/null)"; then
        [ -n "$current_interpreter" ] || return 0
        patchelf --set-interpreter "$loader_path" "$elf"
    fi
    patchelf --force-rpath --set-rpath "$rpath" "$elf"
}

wrap_fixed_address_executable() {
    local elf="$1"
    local loader_path="$2"
    local relative_path="${elf#"$APP_ROOT"/}"
    local wrapped_elf="${elf}.glibc-bin"
    local target_elf="$INSTALL_ROOT/${relative_path}.glibc-bin"

    mv "$elf" "$wrapped_elf"
    cat > "$elf" <<EOF
#!/bin/sh
exec "$loader_path" --library-path "$TARGET_RUNTIME_LIB" "$target_elf" "\$@"
EOF
    chmod 0755 "$elf" "$wrapped_elf"
}

restore_wrapped_executables() {
    local wrapped_elf
    local elf

    while IFS= read -r -d '' wrapped_elf; do
        elf="${wrapped_elf%.glibc-bin}"
        rm -f "$elf"
        mv "$wrapped_elf" "$elf"
    done < <(find "$APP_ROOT" -type f -name '*.glibc-bin' -print0)
}

find_app_elf_candidates() {
    find "$APP_ROOT" \
        -path "$RUNTIME_ROOT" -prune -o \
        -type f \( -perm /111 -o -name '*.node' -o -name '*.so' -o -name '*.so.*' \) \
        -print0
}

main() {
    ensure_app_layout
    command -v cmp >/dev/null 2>&1 || error "cmp is required"
    command -v ldconfig >/dev/null 2>&1 || error "ldconfig is required"
    command -v lddtree >/dev/null 2>&1 || error "lddtree is required (pax-utils)"
    command -v od >/dev/null 2>&1 || error "od is required"
    command -v patchelf >/dev/null 2>&1 || error "patchelf is required"
    command -v readelf >/dev/null 2>&1 || error "readelf is required"
    command -v sha256sum >/dev/null 2>&1 || error "sha256sum is required"

    [ -x "$APP_ROOT/electron" ] || error "Missing Electron binary: $APP_ROOT/electron"
    case "$INSTALL_ROOT" in
        /opt/*) ;;
        *) error "postmarketOS install root must be below /opt: $INSTALL_ROOT" ;;
    esac

    local machine
    local loader_name
    local loader_source
    local target_loader
    local elf
    local elf_type
    local candidate_machine
    local -a app_elfs=()

    restore_wrapped_executables

    machine="$(elf_machine "$APP_ROOT/electron")"
    loader_name="$(loader_name_for_machine "$machine")"
    loader_source="$(loader_source_for_machine "$machine")"
    [ -f "$loader_source" ] || error "Electron interpreter is missing: $loader_source"

    info "Staging private glibc runtime for $machine"
    rm -rf "$RUNTIME_ROOT"
    mkdir -p "$RUNTIME_LIB"

    while IFS= read -r -d '' elf; do
        is_elf_file "$elf" || continue
        candidate_machine="$(elf_machine "$elf")"
        [ "$candidate_machine" = "$machine" ] || continue
        app_elfs+=("$elf")
        collect_elf_dependencies "$elf"
    done < <(find_app_elf_candidates)

    copy_runtime_library "$loader_source"
    stage_dri_drivers "$machine"
    for elf in libc.so.6 libdl.so.2 libm.so.6 libnss_dns.so.2 libnss_files.so.2 \
        libpthread.so.0 libresolv.so.2 librt.so.1 libutil.so.1 libX11-xcb.so.1; do
        copy_optional_glibc_service_library "$elf"
    done
    if [ -d /usr/share/common-licenses ]; then
        mkdir -p "$RUNTIME_LICENSES/common"
        cp -aL /usr/share/common-licenses/. "$RUNTIME_LICENSES/common/"
    fi

    [ -f "$RUNTIME_LIB/$loader_name" ] || error "Private glibc loader was not staged"
    [ -f "$RUNTIME_LIB/libc.so.6" ] || error "Private glibc libc was not staged"
    target_loader="$TARGET_RUNTIME_LIB/$loader_name"

    for elf in "${app_elfs[@]}"; do
        elf_type="$(readelf -h "$elf" | awk -F: '$1 ~ /Type/ {gsub(/^[[:space:]]+/, "", $2); print $2; exit}')"
        if [[ "$elf_type" == EXEC* ]] && patchelf --print-interpreter "$elf" >/dev/null 2>&1; then
            wrap_fixed_address_executable "$elf" "$target_loader"
        else
            patch_app_elf "$elf" "$target_loader"
        fi
    done

    (
        cd "$RUNTIME_ROOT"
        manifest_roots=(lib)
        [ ! -d dri ] || manifest_roots+=(dri)
        [ ! -d licenses ] || manifest_roots+=(licenses)
        find "${manifest_roots[@]}" -type f -printf '%p\0' | sort -z | xargs -0 -r sha256sum
    ) > "$MANIFEST"
    chmod 0644 "$MANIFEST" "$RUNTIME_LIB"/*
    if [ -d "$RUNTIME_DRI" ]; then
        chmod 0644 "$RUNTIME_DRI"/*
    fi
    if [ -d "$RUNTIME_LICENSES" ]; then
        find "$RUNTIME_LICENSES" -type f -exec chmod 0644 {} +
    fi
    chmod 0755 "$RUNTIME_LIB/$loader_name"

    info "Staged $(wc -l < "$MANIFEST") private glibc runtime files in $RUNTIME_ROOT"
}

main "$@"
