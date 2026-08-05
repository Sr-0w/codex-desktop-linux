#!/bin/bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=scripts/lib/package-common.sh
. "$REPO_DIR/scripts/lib/package-common.sh"

# Used by helpers sourced from package-common.sh.
# shellcheck disable=SC2034
APP_DIR="${APP_DIR_OVERRIDE:-$REPO_DIR/codex-app}"
DIST_DIR="${DIST_DIR_OVERRIDE:-$REPO_DIR/dist}"
APKBUILD_TEMPLATE="$REPO_DIR/packaging/postmarketos/APKBUILD.template"
POST_INSTALL_TEMPLATE="$REPO_DIR/packaging/postmarketos/codex-desktop.post-install"
DESKTOP_TEMPLATE="$REPO_DIR/packaging/linux/codex-desktop.desktop"
SERVICE_TEMPLATE="$REPO_DIR/packaging/linux/codex-update-manager.service"
ICON_SOURCE="$REPO_DIR/assets/codex-linux.png"
PACKAGED_RUNTIME_TEMPLATE="$REPO_DIR/packaging/linux/codex-packaged-runtime.sh"

PACKAGE_NAME="${PACKAGE_NAME:-codex-desktop}"
PACKAGE_VERSION="${PACKAGE_VERSION:-$(date -u +%Y.%m.%d.%H%M%S)}"
PACKAGE_WITH_UPDATER="${PACKAGE_WITH_UPDATER:-1}"
UPDATER_BINARY_SOURCE="${UPDATER_BINARY_SOURCE:-$REPO_DIR/target/aarch64-unknown-linux-musl/release/codex-update-manager}"
UPDATER_SERVICE_SOURCE="${UPDATER_SERVICE_SOURCE:-$SERVICE_TEMPLATE}"
PACKAGED_RUNTIME_SOURCE="${PACKAGED_RUNTIME_SOURCE:-$PACKAGED_RUNTIME_TEMPLATE}"
POSTMARKETOS_ABUILD_IMAGE="${POSTMARKETOS_ABUILD_IMAGE:-alpine:3.22}"
POSTMARKETOS_ARCH="${POSTMARKETOS_ARCH:-aarch64}"

apk_package_version() {
    local version="${PACKAGE_VERSION%%+*}"
    case "$version" in
        [0-9][0-9][0-9][0-9].[0-9][0-9].[0-9][0-9].[0-9][0-9][0-9][0-9][0-9][0-9])
            printf '%s\n' "$version"
            ;;
        *)
            error "postmarketOS package version must look like YYYY.MM.DD.HHMMSS, got: $PACKAGE_VERSION"
            ;;
    esac
}

validate_architecture() {
    local host_arch
    case "$(uname -m)" in
        aarch64|arm64) host_arch="aarch64" ;;
        x86_64) host_arch="x86_64" ;;
        *) error "Unsupported postmarketOS build architecture: $(uname -m)" ;;
    esac

    case "$POSTMARKETOS_ARCH" in
        aarch64|x86_64) ;;
        *) error "POSTMARKETOS_ARCH must be aarch64 or x86_64" ;;
    esac
    [ "$host_arch" = "$POSTMARKETOS_ARCH" ] || error \
        "postmarketOS packages must be built natively ($host_arch host, $POSTMARKETOS_ARCH target)"
}

validate_updater_binary() {
    package_with_updater_enabled || return 0
    ensure_file_exists "$UPDATER_BINARY_SOURCE" "musl updater binary"
    [ -x "$UPDATER_BINARY_SOURCE" ] || error "Updater binary is not executable: $UPDATER_BINARY_SOURCE"

    local interpreter
    interpreter="$(patchelf --print-interpreter "$UPDATER_BINARY_SOURCE" 2>/dev/null || true)"
    case "$interpreter" in
        ""|*/ld-musl-*.so.1) ;;
        *) error "postmarketOS updater must be static or musl-linked, got interpreter: $interpreter" ;;
    esac

    local machine
    machine="$(readelf -h "$UPDATER_BINARY_SOURCE" | awk -F: '$1 ~ /Machine/ {gsub(/^[[:space:]]+/, "", $2); print $2; exit}')"
    case "$POSTMARKETOS_ARCH:$machine" in
        aarch64:*AArch64*|x86_64:*X86-64*) ;;
        *) error "Updater architecture does not match $POSTMARKETOS_ARCH: $machine" ;;
    esac
}

write_postmarketos_launcher() {
    local root="$1"

    cat > "$root/usr/bin/$PACKAGE_NAME" <<SCRIPT
#!/bin/bash
export CODEX_LINUX_RENDERING_MODE="\${CODEX_LINUX_RENDERING_MODE:-wayland-gpu}"
if [ -d "/opt/$PACKAGE_NAME/.codex-linux/glibc-runtime/dri" ]; then
    export LIBGL_DRIVERS_PATH="/opt/$PACKAGE_NAME/.codex-linux/glibc-runtime/dri"
fi
exec "/opt/$PACKAGE_NAME/start.sh" \
    --wayland \
    --enable-wayland-ime \
    --wayland-text-input-version=1 \
    --disable-features=Vulkan \
    "\$@"
SCRIPT
    chmod 0755 "$root/usr/bin/$PACKAGE_NAME"
}

render_apkbuild() {
    local target="$1"
    local staging_root="$2"
    local package_version="$3"
    local escaped_staging

    escaped_staging="$(sed_escape_replacement "$staging_root")"
    sed \
        -e "s/__PACKAGE_NAME__/$PACKAGE_NAME/g" \
        -e "s/__PKGVER__/$package_version/g" \
        -e "s/__ARCH__/$POSTMARKETOS_ARCH/g" \
        -e "s/__STAGING_DIR__/$escaped_staging/g" \
        "$APKBUILD_TEMPLATE" > "$target"
}

run_abuild_native() {
    local build_root="$1"
    local home="$build_root/home"

    mkdir -p "$home"
    HOME="$home" abuild-keygen -a -n >/dev/null
    HOME="$home" REPODEST="$build_root/packages" abuild -F rootpkg
}

run_abuild_bwrap() {
    local build_root="$1"
    local rootfs="$2"

    [ -x "$rootfs/usr/bin/abuild" ] || error "Alpine rootfs does not contain abuild: $rootfs"
    # The inner shell expands the mounted-build variables.
    # shellcheck disable=SC2016
    bwrap \
        --unshare-user \
        --uid 0 \
        --gid 0 \
        --bind "$rootfs" / \
        --dev /dev \
        --proc /proc \
        --bind "$build_root" "$build_root" \
        --setenv HOME "$build_root/home" \
        --setenv BUILD_ROOT "$build_root" \
        --setenv PATH /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        --chdir "$build_root" \
        /bin/sh -ec '
            mkdir -p "$HOME" "$BUILD_ROOT/packages"
            abuild-keygen -a -n >/dev/null
            REPODEST="$BUILD_ROOT/packages" abuild -F rootpkg
        '
}

container_engine() {
    if command -v docker >/dev/null 2>&1; then
        command -v docker
    elif command -v podman >/dev/null 2>&1; then
        command -v podman
    else
        return 1
    fi
}

run_abuild_container() {
    local build_root="$1"
    local engine
    local uid
    local gid

    engine="$(container_engine)" || error "abuild or Docker/Podman is required to build the APK"
    uid="$(id -u)"
    gid="$(id -g)"

    # The container shell expands these injected environment variables.
    # shellcheck disable=SC2016
    "$engine" run --rm \
        -e BUILD_ROOT="$build_root" \
        -e BUILDER_UID="$uid" \
        -e BUILDER_GID="$gid" \
        -e PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        -v "$build_root:$build_root" \
        -w "$build_root" \
        "$POSTMARKETOS_ABUILD_IMAGE" \
        /bin/sh -euxc '
            /sbin/apk add --no-cache alpine-sdk
            addgroup -g "$BUILDER_GID" codex-build
            adduser -D -u "$BUILDER_UID" -G codex-build codex-build
            chown -R "$BUILDER_UID:$BUILDER_GID" "$BUILD_ROOT"
            su codex-build -s /bin/sh -c "
                export HOME=\"$BUILD_ROOT/home\"
                mkdir -p \"\$HOME\"
                cd \"$BUILD_ROOT\"
                abuild-keygen -a -n >/dev/null
                REPODEST=\"$BUILD_ROOT/packages\" abuild -F rootpkg
            "
        '
}

main() {
    validate_architecture
    ensure_app_layout
    ensure_file_exists "$APKBUILD_TEMPLATE" "APKBUILD template"
    ensure_file_exists "$POST_INSTALL_TEMPLATE" "APK post-install template"
    ensure_file_exists "$DESKTOP_TEMPLATE" "desktop template"
    ensure_file_exists "$ICON_SOURCE" "icon"
    ensure_file_exists "$PACKAGED_RUNTIME_SOURCE" "packaged launcher runtime helper"
    command -v patchelf >/dev/null 2>&1 || error "patchelf is required"
    command -v readelf >/dev/null 2>&1 || error "readelf is required"
    validate_updater_binary

    local package_version
    local build_root
    local staging_root
    local app_root
    local package_file
    package_version="$(apk_package_version)"
    build_root="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$build_root'" EXIT
    staging_root="$build_root/staging"
    app_root="$staging_root/opt/$PACKAGE_NAME"

    stage_common_package_files "$staging_root"
    rm -rf "$staging_root/usr/lib/systemd"
    write_postmarketos_launcher "$staging_root"
    run_linux_feature_package_hooks "$staging_root" "apk"
    "$REPO_DIR/scripts/stage-postmarketos-runtime.sh" "$app_root" "/opt/$PACKAGE_NAME"
    normalize_package_payload_permissions "$staging_root"
    restore_linux_feature_payload_permissions "$staging_root"

    mkdir -p "$staging_root/usr/share/licenses/$PACKAGE_NAME"
    cp "$REPO_DIR/LICENSE" "$staging_root/usr/share/licenses/$PACKAGE_NAME/wrapper-MIT.txt"
    cp "$app_root/LICENSE" "$staging_root/usr/share/licenses/$PACKAGE_NAME/upstream-Electron.txt"
    cp "$app_root/LICENSES.chromium.html" \
        "$staging_root/usr/share/licenses/$PACKAGE_NAME/chromium.html"

    render_apkbuild "$build_root/APKBUILD" "$staging_root" "$package_version"
    cp "$POST_INSTALL_TEMPLATE" "$build_root/$PACKAGE_NAME.post-install"
    cp "$POST_INSTALL_TEMPLATE" "$build_root/$PACKAGE_NAME.post-upgrade"

    if command -v abuild >/dev/null 2>&1; then
        run_abuild_native "$build_root"
    elif [ -n "${POSTMARKETOS_ALPINE_ROOTFS:-}" ] && command -v bwrap >/dev/null 2>&1; then
        run_abuild_bwrap "$build_root" "$POSTMARKETOS_ALPINE_ROOTFS"
    else
        run_abuild_container "$build_root"
    fi

    package_file="$(find "$build_root/packages" -type f -name "$PACKAGE_NAME-$package_version-r0.apk" -print -quit)"
    [ -f "$package_file" ] || error "abuild did not produce the expected APK"
    mkdir -p "$DIST_DIR"
    cp "$package_file" "$DIST_DIR/$PACKAGE_NAME-$package_version-$POSTMARKETOS_ARCH.apk"
    ln -sfn "$PACKAGE_NAME-$package_version-$POSTMARKETOS_ARCH.apk" \
        "$DIST_DIR/$PACKAGE_NAME-latest.apk"
    info "Built postmarketOS package: $DIST_DIR/$PACKAGE_NAME-$package_version-$POSTMARKETOS_ARCH.apk"
    printf '%s\n' "$DIST_DIR/$PACKAGE_NAME-$package_version-$POSTMARKETOS_ARCH.apk"
}

main "$@"
