# Gentoo Overlay Packaging

`scripts/build-gentoo-bin.sh` creates a self-contained Portage overlay artifact
for `app-editors/codex-desktop-bin`.

The release artifact contains:

- `overlay/`: a local Portage repository named `codex-desktop-linux`
- `distfiles/`: the prebuilt payload consumed by the ebuild
- `install-gentoo.sh`: a helper that installs the overlay under
  `/var/db/repos/codex-desktop-linux`, copies the distfile into Portage's
  `DISTDIR`, writes `/etc/portage/repos.conf/codex-desktop-linux.conf`, and runs
  `emerge --oneshot`

The Gentoo package installs the same runtime payload as the other native
formats, but it intentionally omits the `systemd --user` unit from the live
filesystem. On OpenRC and other non-systemd sessions, the packaged launcher runs
`codex-update-manager check-now --if-stale` directly in the background.
