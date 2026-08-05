#!/bin/bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=scripts/lib/package-common.sh
. "$REPO_DIR/scripts/lib/package-common.sh"

# Read by the shared package helpers sourced above.
# shellcheck disable=SC2034
APP_DIR="${APP_DIR_OVERRIDE:-$REPO_DIR/codex-app}"
DIST_DIR="${DIST_DIR_OVERRIDE:-$REPO_DIR/dist}"
EBUILD_TEMPLATE="$REPO_DIR/packaging/gentoo/codex-desktop-bin.ebuild.template"
GENTOO_METADATA_TEMPLATE="$REPO_DIR/packaging/gentoo/metadata.xml"
DESKTOP_TEMPLATE="$REPO_DIR/packaging/linux/codex-desktop.desktop"
SERVICE_TEMPLATE="$REPO_DIR/packaging/linux/codex-update-manager.service"
USER_SERVICE_HELPER_TEMPLATE="$REPO_DIR/packaging/linux/codex-update-manager-user-service.sh"
ICON_SOURCE="$REPO_DIR/assets/codex-linux.png"
PACKAGED_RUNTIME_TEMPLATE="$REPO_DIR/packaging/linux/codex-packaged-runtime.sh"

PACKAGE_NAME="${PACKAGE_NAME:-codex-desktop}"
PACKAGE_VERSION="${PACKAGE_VERSION:-$(date -u +%Y.%m.%d.%H%M%S)}"
MAX_BUILD_THREADS="${MAX_BUILD_THREADS:-0}"
UPDATER_BINARY_SOURCE="${UPDATER_BINARY_SOURCE:-$REPO_DIR/target/release/codex-update-manager}"
UPDATER_SERVICE_SOURCE="${UPDATER_SERVICE_SOURCE:-$SERVICE_TEMPLATE}"
PACKAGED_RUNTIME_SOURCE="${PACKAGED_RUNTIME_SOURCE:-$PACKAGED_RUNTIME_TEMPLATE}"

GENTOO_REPO_NAME="${GENTOO_REPO_NAME:-codex-desktop-linux}"
GENTOO_CATEGORY="${GENTOO_CATEGORY:-app-editors}"
GENTOO_PN="${GENTOO_PN:-codex-desktop-bin}"

validate_max_build_threads() {
    case "$MAX_BUILD_THREADS" in
        ""|*[!0-9]*)
            error "MAX_BUILD_THREADS must be 0 or a positive integer"
            ;;
    esac
}

gentoo_package_version() {
    local base="${PACKAGE_VERSION%%+*}"
    case "$base" in
        [0-9][0-9][0-9][0-9].[0-9][0-9].[0-9][0-9].[0-9][0-9][0-9][0-9][0-9][0-9])
            printf '%s\n' "$base"
            ;;
        *)
            error "Gentoo package version must look like YYYY.MM.DD.HHMMSS, got: $PACKAGE_VERSION"
            ;;
    esac
}

map_arch() {
    case "$(uname -m)" in
        x86_64) echo "amd64" ;;
        aarch64|arm64) echo "arm64" ;;
        *) error "Unsupported Gentoo architecture: $(uname -m)" ;;
    esac
}

zstd_command() {
    if [ "$MAX_BUILD_THREADS" = "0" ]; then
        printf '%s\n' "zstd -T0 -19"
    else
        printf '%s\n' "zstd -T$MAX_BUILD_THREADS -19"
    fi
}

verify_zstd_tar_archive() {
    local archive="$1"

    zstd -t -- "$archive" >/dev/null
    tar -I zstd -tf "$archive" >/dev/null
}

build_verified_zstd_tar() {
    local source_root="$1"
    local target_file="$2"
    local temporary_root="$3"
    shift 3

    local temporary_file
    temporary_file="$temporary_root/.$(basename "$target_file").tmp"
    local attempt
    for attempt in 1 2; do
        if tar -C "$source_root" -I "$(zstd_command)" -cf "$temporary_file" "$@" \
            && verify_zstd_tar_archive "$temporary_file"; then
            mv -f "$temporary_file" "$target_file"
            return 0
        fi
        warn "Archive verification failed for $(basename "$target_file") (attempt $attempt/2)"
    done
    error "Could not produce a valid archive: $target_file"
}

write_manifest() {
    local distfile="$1"
    local manifest="$2"
    local distfile_name
    local size
    local blake2b
    local sha512

    command -v b2sum >/dev/null 2>&1 || error "b2sum is required to write the Gentoo Manifest"
    command -v sha512sum >/dev/null 2>&1 || error "sha512sum is required to write the Gentoo Manifest"

    distfile_name="$(basename "$distfile")"
    size="$(wc -c <"$distfile" | tr -d '[:space:]')"
    blake2b="$(b2sum "$distfile" | awk '{print $1}')"
    sha512="$(sha512sum "$distfile" | awk '{print $1}')"
    printf 'DIST %s %s BLAKE2B %s SHA512 %s\n' \
        "$distfile_name" "$size" "$blake2b" "$sha512" >"$manifest"
}

write_install_helper() {
    local target="$1"

    cat >"$target" <<'SCRIPT'
#!/bin/sh
set -eu

bundle_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_name="$(cat "$bundle_dir/metadata/repo-name")"
category="$(cat "$bundle_dir/metadata/category")"
package_name="$(cat "$bundle_dir/metadata/package-name")"
package_version="$(cat "$bundle_dir/metadata/package-version")"
distfile="$(cat "$bundle_dir/metadata/distfile")"
bundle_arch="$(cat "$bundle_dir/metadata/architecture")"
atom="=$category/$package_name-$package_version"

case "$(uname -m)" in
    x86_64|amd64) host_arch="amd64" ;;
    aarch64|arm64) host_arch="arm64" ;;
    *) host_arch="unsupported" ;;
esac
if [ "$host_arch" != "$bundle_arch" ]; then
    echo "This bundle targets $bundle_arch, but this system is $host_arch ($(uname -m))." >&2
    exit 1
fi

if ! command -v emerge >/dev/null 2>&1; then
    echo "emerge is required to install $atom" >&2
    exit 1
fi

distdir="$(portageq envvar DISTDIR 2>/dev/null | sed -n '1p' || true)"
[ -n "$distdir" ] || distdir="/var/cache/distfiles"
repo_root="/var/db/repos/$repo_name"
repos_conf="/etc/portage/repos.conf/$repo_name.conf"

mkdir -p "$distdir" "$(dirname "$repo_root")" "$(dirname "$repos_conf")"
cp "$bundle_dir/distfiles/$distfile" "$distdir/$distfile"
chmod 0644 "$distdir/$distfile"
rm -rf "$repo_root"
mkdir -p "$repo_root"
cp -a "$bundle_dir/overlay/." "$repo_root/"

cat >"$repos_conf" <<CONF
[$repo_name]
location = $repo_root
masters = gentoo
auto-sync = no
CONF

if command -v egencache >/dev/null 2>&1; then
    egencache --repo "$repo_name" --update >/dev/null 2>&1 || true
fi

emerge --oneshot --verbose "$atom"
SCRIPT
    chmod 0755 "$target"
}

main() {
    validate_max_build_threads
    ensure_app_layout
    ensure_file_exists "$EBUILD_TEMPLATE" "Gentoo ebuild template"
    ensure_file_exists "$GENTOO_METADATA_TEMPLATE" "Gentoo metadata template"
    ensure_file_exists "$DESKTOP_TEMPLATE" "desktop template"
    ensure_file_exists "$ICON_SOURCE" "icon"
    if package_with_updater_enabled; then
        ensure_file_exists "$UPDATER_SERVICE_SOURCE" "updater service template"
        ensure_file_exists "$USER_SERVICE_HELPER_TEMPLATE" "updater user service helper"
        ensure_file_exists "$PACKAGED_RUNTIME_SOURCE" "packaged launcher runtime helper"
    else
        info "Building Gentoo package without codex-update-manager (PACKAGE_WITH_UPDATER=0)"
    fi
    command -v tar >/dev/null 2>&1 || error "tar is required"
    command -v zstd >/dev/null 2>&1 || error "zstd is required"

    ensure_updater_binary

    local gentoo_pv gentoo_arch p distfile_name build_root image_root payload_root bundle_root
    local payload_file artifact_file overlay_root package_root ebuild_file manifest_file
    gentoo_pv="$(gentoo_package_version)"
    gentoo_arch="$(map_arch)"
    p="$GENTOO_PN-$gentoo_pv"
    distfile_name="$p.tar.zst"

    build_root="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$build_root'" EXIT

    image_root="$build_root/image"
    payload_root="$build_root/payload"
    bundle_root="$build_root/codex-desktop-linux-gentoo"
    overlay_root="$bundle_root/overlay"
    package_root="$overlay_root/$GENTOO_CATEGORY/$GENTOO_PN"

    stage_common_package_files "$image_root"
    stage_optional_update_builder_bundle "$image_root"
    write_launcher_stub "$image_root"
    if package_with_updater_enabled; then
        rm -rf "$image_root/usr/lib/systemd"
    fi
    run_linux_feature_package_hooks "$image_root" "gentoo"
    normalize_package_payload_permissions "$image_root"
    restore_linux_feature_payload_permissions "$image_root"

    mkdir -p "$DIST_DIR" "$payload_root"
    cp -a "$image_root" "$payload_root/image"
    payload_file="$DIST_DIR/$distfile_name"
    info "Building Gentoo distfile $payload_file"
    build_verified_zstd_tar "$payload_root" "$payload_file" "$build_root" image

    mkdir -p \
        "$package_root" \
        "$overlay_root/profiles" \
        "$overlay_root/metadata" \
        "$bundle_root/distfiles" \
        "$bundle_root/metadata"
    printf '%s\n' "$GENTOO_REPO_NAME" >"$overlay_root/profiles/repo_name"
    cat >"$overlay_root/metadata/layout.conf" <<EOF
masters = gentoo
thin-manifests = true
sign-manifests = false
EOF
    cp "$GENTOO_METADATA_TEMPLATE" "$package_root/metadata.xml"
    ebuild_file="$package_root/$p.ebuild"
    cp "$EBUILD_TEMPLATE" "$ebuild_file"
    sed -i "s/__KEYWORDS__/$gentoo_arch/g" "$ebuild_file"
    if ! package_with_updater_enabled; then
        sed -i '/sys-auth\/polkit/d' "$ebuild_file"
    fi
    manifest_file="$package_root/Manifest"
    cp "$payload_file" "$bundle_root/distfiles/$distfile_name"
    verify_zstd_tar_archive "$bundle_root/distfiles/$distfile_name"
    write_manifest "$bundle_root/distfiles/$distfile_name" "$manifest_file"

    printf '%s\n' "$GENTOO_REPO_NAME" >"$bundle_root/metadata/repo-name"
    printf '%s\n' "$GENTOO_CATEGORY" >"$bundle_root/metadata/category"
    printf '%s\n' "$GENTOO_PN" >"$bundle_root/metadata/package-name"
    printf '%s\n' "$gentoo_pv" >"$bundle_root/metadata/package-version"
    printf '%s\n' "$distfile_name" >"$bundle_root/metadata/distfile"
    printf '%s\n' "$gentoo_arch" >"$bundle_root/metadata/architecture"
    printf '%s\n' "=$GENTOO_CATEGORY/$GENTOO_PN-$gentoo_pv" >"$bundle_root/metadata/atom"
    printf '%s\n' "$PACKAGE_VERSION" >"$bundle_root/metadata/upstream-package-version"
    write_install_helper "$bundle_root/install-gentoo.sh"

    artifact_file="$DIST_DIR/${PACKAGE_NAME}-${gentoo_pv}-${gentoo_arch}.gentoo.tar.zst"
    info "Building Gentoo overlay artifact $artifact_file"
    build_verified_zstd_tar \
        "$build_root" \
        "$artifact_file" \
        "$build_root" \
        "$(basename "$bundle_root")"
    ln -sfn "$(basename "$artifact_file")" "$DIST_DIR/${PACKAGE_NAME}-latest.gentoo.tar.zst"

    info "Built Gentoo overlay artifact: $artifact_file"
    printf '%s\n' "$artifact_file"
}

main "$@"
