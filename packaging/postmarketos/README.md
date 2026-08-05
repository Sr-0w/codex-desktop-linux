# postmarketOS Packaging

`scripts/build-postmarketos.sh` creates an aarch64 APK for postmarketOS and
Plasma Mobile. The upstream Electron runtime is glibc-based, while postmarketOS
uses musl, so the package keeps a private, app-local glibc runtime instead of
installing or replacing the system C library.

The launcher selects native Wayland, Wayland text input, and the GPU rendering
profile used by Plasma Mobile. Vulkan is disabled so the glibc Electron process
does not load a musl Vulkan driver from the host. Raspberry Pi Mesa DRI drivers
can be staged with the private runtime when available on the ARM64 build host.

The APK is built with Alpine 3.22 `abuild` so it remains readable by apk-tools
2.x as well as newer apk-tools releases. Release APKs are signed with an
ephemeral CI key and installed with `apk add --allow-untrusted`; their published
`.sha256` file is verified before an automatic update is accepted.
