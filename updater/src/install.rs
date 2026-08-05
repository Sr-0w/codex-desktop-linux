//! Installation helpers for privileged and non-privileged package application.

use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

const PACKAGE_NAME: &str = "codex-desktop";
const GENTOO_CATEGORY: &str = "app-editors";
const GENTOO_PACKAGE_NAME: &str = "codex-desktop-bin";
const GENTOO_REPO_NAME: &str = "codex-desktop-linux";
const INSTALLED_UPDATER_BINARY: &str = "/usr/bin/codex-update-manager";
const APT_CANDIDATES: &[&str] = &["/usr/bin/apt", "/bin/apt"];
const DNF_CANDIDATES: &[&str] = &["/usr/bin/dnf", "/bin/dnf", "/usr/bin/dnf5", "/bin/dnf5"];
const DPKG_CANDIDATES: &[&str] = &["/usr/bin/dpkg", "/bin/dpkg"];
const DPKG_DEB_CANDIDATES: &[&str] = &["/usr/bin/dpkg-deb", "/bin/dpkg-deb"];
const DPKG_QUERY_CANDIDATES: &[&str] = &["/usr/bin/dpkg-query", "/bin/dpkg-query"];
const RPM_CANDIDATES: &[&str] = &["/usr/bin/rpm", "/bin/rpm"];
const ZYPPER_CANDIDATES: &[&str] = &["/usr/bin/zypper", "/bin/zypper"];
const PACMAN_CANDIDATES: &[&str] = &["/usr/bin/pacman", "/bin/pacman"];
const EMERGE_CANDIDATES: &[&str] = &["/usr/bin/emerge", "/bin/emerge"];
const PORTAGEQ_CANDIDATES: &[&str] = &["/usr/bin/portageq", "/bin/portageq"];
const TAR_CANDIDATES: &[&str] = &["/usr/bin/tar", "/bin/tar"];
const VERCMP_CANDIDATES: &[&str] = &["/usr/bin/vercmp", "/bin/vercmp"];
const GENTOO_PACKAGE_SUFFIX: &str = ".gentoo.tar.zst";
const PACMAN_PACKAGE_SUFFIXES: &[&str] = &[
    ".pkg.tar.zst",
    ".pkg.tar.xz",
    ".pkg.tar.gz",
    ".pkg.tar.bz2",
    ".pkg.tar.lz",
    ".pkg.tar.lz4",
    ".pkg.tar.lz5",
];

/// The native package format in use on the current system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Deb,
    Rpm,
    Pacman,
    Gentoo,
}

impl PackageKind {
    pub fn detect() -> Self {
        detect_package_kind(PackageDetection {
            has_pacman: program_exists(PACMAN_CANDIDATES, "pacman"),
            has_emerge: program_exists(EMERGE_CANDIDATES, "emerge"),
            has_dpkg: program_exists(DPKG_CANDIDATES, "dpkg"),
            has_rpm: program_exists(RPM_CANDIDATES, "rpm"),
            pacman_installed: installed_pacman_version() != "unknown",
            gentoo_installed: installed_gentoo_version() != "unknown",
            deb_installed: installed_deb_version() != "unknown",
            rpm_installed: installed_rpm_version() != "unknown",
            os_release: os_release_fields(),
        })
    }

    pub fn from_path(path: &Path) -> Self {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if is_gentoo_package_file_name(file_name) {
            return Self::Gentoo;
        }
        if is_pacman_package_file_name(file_name) {
            return Self::Pacman;
        }

        match path.extension().and_then(|e| e.to_str()) {
            Some("rpm") => Self::Rpm,
            _ => Self::Deb,
        }
    }
}

#[derive(Default)]
struct PackageDetection {
    has_pacman: bool,
    has_emerge: bool,
    has_dpkg: bool,
    has_rpm: bool,
    pacman_installed: bool,
    gentoo_installed: bool,
    deb_installed: bool,
    rpm_installed: bool,
    os_release: Option<(String, String)>,
}

fn detect_package_kind(detection: PackageDetection) -> PackageKind {
    if let Some((id, id_like)) = detection.os_release {
        let fields = [id.as_str(), id_like.as_str()];
        if os_release_matches(&fields, &["gentoo"]) {
            return PackageKind::Gentoo;
        }
        if os_release_matches(
            &fields,
            &["arch", "archlinux", "manjaro", "endeavouros", "artix"],
        ) {
            return PackageKind::Pacman;
        }
        if os_release_matches(
            &fields,
            &[
                "debian",
                "ubuntu",
                "linuxmint",
                "pop",
                "elementary",
                "zorin",
            ],
        ) {
            return PackageKind::Deb;
        }
        if os_release_matches(
            &fields,
            &[
                "fedora",
                "rhel",
                "centos",
                "rocky",
                "almalinux",
                "ol",
                "sles",
                "suse",
                "opensuse",
            ],
        ) {
            return PackageKind::Rpm;
        }
    }

    if detection.pacman_installed {
        return PackageKind::Pacman;
    }
    if detection.gentoo_installed {
        return PackageKind::Gentoo;
    }
    if detection.deb_installed {
        return PackageKind::Deb;
    }
    if detection.rpm_installed {
        return PackageKind::Rpm;
    }

    if detection.has_dpkg {
        PackageKind::Deb
    } else if detection.has_rpm {
        PackageKind::Rpm
    } else if detection.has_pacman {
        PackageKind::Pacman
    } else if detection.has_emerge {
        PackageKind::Gentoo
    } else {
        PackageKind::Deb
    }
}

fn os_release_fields() -> Option<(String, String)> {
    let contents = fs::read_to_string("/etc/os-release").ok()?;
    let mut id = String::new();
    let mut id_like = String::new();

    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("ID=") {
            id = trim_os_release_value(value).to_ascii_lowercase();
        } else if let Some(value) = line.strip_prefix("ID_LIKE=") {
            id_like = trim_os_release_value(value).to_ascii_lowercase();
        }
    }

    Some((id, id_like))
}

fn trim_os_release_value(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

fn os_release_matches(fields: &[&str], expected: &[&str]) -> bool {
    fields.iter().any(|field| {
        field
            .split_whitespace()
            .any(|token| expected.contains(&token))
    })
}

/// Returns the currently installed package version when available.
pub fn installed_package_version() -> String {
    match PackageKind::detect() {
        PackageKind::Deb => installed_deb_version(),
        PackageKind::Rpm => installed_rpm_version(),
        PackageKind::Pacman => installed_pacman_version(),
        PackageKind::Gentoo => installed_gentoo_version(),
    }
}

/// Returns whether the primary native package still appears to be installed.
pub fn is_primary_package_installed() -> bool {
    installed_package_version() != "unknown"
}

fn installed_deb_version() -> String {
    installed_version_from_command(
        &program_path(DPKG_QUERY_CANDIDATES, "dpkg-query"),
        &["-W", "-f=${Version}", PACKAGE_NAME],
    )
}

fn installed_rpm_version() -> String {
    installed_version_from_command(
        &program_path(RPM_CANDIDATES, "rpm"),
        &["-q", "--queryformat", "%{VERSION}-%{RELEASE}", PACKAGE_NAME],
    )
}

fn installed_pacman_version() -> String {
    match Command::new(program_path(PACMAN_CANDIDATES, "pacman"))
        .args(["-Q", PACKAGE_NAME])
        .output()
    {
        Ok(output) if output.status.success() => parse_pacman_installed_version(output.stdout),
        _ => "unknown".to_string(),
    }
}

fn installed_gentoo_version() -> String {
    installed_gentoo_version_from_vdb(Path::new("/var/db/pkg"))
}

fn installed_gentoo_version_from_vdb(vdb_root: &Path) -> String {
    let category_dir = vdb_root.join(GENTOO_CATEGORY);
    let Ok(entries) = fs::read_dir(category_dir) else {
        return "unknown".to_string();
    };
    let prefix = format!("{GENTOO_PACKAGE_NAME}-");
    let mut versions = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| name.strip_prefix(&prefix).map(str::to_string))
        .filter(|version| !version.is_empty())
        .collect::<Vec<_>>();
    versions.sort();
    versions.pop().unwrap_or_else(|| "unknown".to_string())
}

/// Installs a rebuilt Debian package on the local machine.
pub fn install_deb(path: &Path) -> Result<()> {
    let stable = stable_validated_package(path)
        .with_context(|| format!("Failed to stabilize Debian package {}", path.display()))?;
    ensure_upgrade_path(stable.path())?;

    if program_exists(APT_CANDIDATES, "apt") {
        let mut command = apt_install_command(stable.path())?;
        run_install(&mut command).context("apt install failed")?;
        return Ok(());
    }

    let mut command = dpkg_install_command(stable.path());
    run_install(&mut command).context("dpkg -i failed")
}

/// Installs a rebuilt RPM package on the local machine.
pub fn install_rpm(path: &Path) -> Result<()> {
    let stable = stable_validated_package(path)
        .with_context(|| format!("Failed to stabilize RPM package {}", path.display()))?;
    ensure_upgrade_path_rpm(stable.path())?;

    if program_exists(DNF_CANDIDATES, "dnf") || program_exists(DNF_CANDIDATES, "dnf5") {
        let mut command = dnf_install_command(stable.path())?;
        run_install(&mut command).context("dnf install failed")?;
        return Ok(());
    }

    if program_exists(ZYPPER_CANDIDATES, "zypper") {
        let mut command = zypper_install_command(stable.path())?;
        run_install(&mut command).context("zypper install failed")?;
        return Ok(());
    }

    let mut command = rpm_install_command(stable.path());
    run_install(&mut command).context("rpm -Uvh failed")
}

/// Installs a rebuilt pacman package on the local machine.
pub fn install_pacman(path: &Path) -> Result<()> {
    let stable = stable_validated_package(path)
        .with_context(|| format!("Failed to stabilize pacman package {}", path.display()))?;
    ensure_upgrade_path_pacman(stable.path())?;

    let mut command = pacman_install_command(stable.path());
    run_install(&mut command).context("pacman -U failed")
}

/// Installs a rebuilt Gentoo overlay artifact on the local machine.
pub fn install_gentoo(path: &Path) -> Result<()> {
    let stable = stable_validated_package(path)
        .with_context(|| format!("Failed to stabilize Gentoo package {}", path.display()))?;
    ensure_upgrade_path_gentoo(stable.path())?;
    merge_gentoo_package(stable.path()).context("Gentoo package install failed")
}

/// Builds the `pkexec` command used for privileged package installation.
pub fn pkexec_command(current_exe: &Path, package_path: &Path) -> Command {
    let updater_binary = updater_binary_for_privileged_install(current_exe);
    let subcommand = match PackageKind::from_path(package_path) {
        PackageKind::Rpm => "install-rpm",
        PackageKind::Deb => "install-deb",
        PackageKind::Pacman => "install-pacman",
        PackageKind::Gentoo => "install-gentoo",
    };
    let mut command = Command::new("pkexec");
    command
        .arg("--disable-internal-agent")
        .arg(updater_binary)
        .arg(subcommand)
        .arg("--path")
        .arg(package_path);
    command
}

fn run_install(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .context("Failed to execute installation command")?;
    anyhow::ensure!(
        status.success(),
        "installation command exited with {status}"
    );
    Ok(())
}

pub(crate) struct StablePackage {
    dir: PathBuf,
    path: PathBuf,
}

impl StablePackage {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StablePackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub(crate) fn stable_validated_package(path: &Path) -> Result<StablePackage> {
    anyhow::ensure!(
        path.is_file(),
        "Package path is not a file: {}",
        path.display()
    );
    let source_path = package_identity_path(path)?;
    let requested_kind = PackageKind::from_path(path);
    let kind = PackageKind::from_path(&source_path);
    anyhow::ensure!(
        requested_kind == kind,
        "Package format changed while resolving {}",
        path.display()
    );
    ensure_codex_package(&source_path)?;

    let dir = create_private_temp_dir()?;
    let stable_path = dir.join(stable_file_name(kind, &source_path)?);
    fs::copy(&source_path, &stable_path).with_context(|| {
        format!(
            "Failed to copy package {} into private staging area",
            source_path.display()
        )
    })?;
    set_private_file_permissions(&stable_path)?;

    anyhow::ensure!(
        PackageKind::from_path(&stable_path) == kind,
        "Package format changed while stabilizing {}",
        path.display()
    );
    ensure_codex_package(&stable_path)?;

    Ok(StablePackage {
        dir,
        path: stable_path,
    })
}

fn package_identity_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path)
        .with_context(|| format!("Failed to resolve package path {}", path.display()))
}

pub(crate) fn ensure_codex_package(path: &Path) -> Result<()> {
    match PackageKind::from_path(path) {
        PackageKind::Deb => ensure_package_name(&deb_package_name(path)?, path),
        PackageKind::Rpm => ensure_package_name(&rpm_package_name(path)?, path),
        PackageKind::Pacman => {
            pacman_package_version(path)?;
            ensure_package_name(&pacman_package_name(path)?, path)
        }
        PackageKind::Gentoo => {
            ensure_gentoo_package(path)?;
            Ok(())
        }
    }
}

fn create_private_temp_dir() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for attempt in 0..100 {
        let dir = base.join(format!(
            "codex-update-manager-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match create_private_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to create private staging directory {}",
                        dir.display()
                    )
                });
            }
        }
    }

    anyhow::bail!("Failed to create a unique private package staging directory")
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to lock down staged package {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn stable_file_name(kind: PackageKind, path: &Path) -> Result<String> {
    match kind {
        PackageKind::Deb => Ok("codex-desktop.deb".to_string()),
        PackageKind::Rpm => Ok("codex-desktop.rpm".to_string()),
        PackageKind::Gentoo => Ok("codex-desktop.gentoo.tar.zst".to_string()),
        PackageKind::Pacman => path
            .file_name()
            .with_context(|| format!("Pacman package path has no file name: {}", path.display()))
            .map(|name| name.to_string_lossy().into_owned()),
    }
}

fn ensure_package_name(package_name: &str, path: &Path) -> Result<()> {
    anyhow::ensure!(
        package_name == PACKAGE_NAME,
        "Refusing to install package {package_name} from {}; expected {PACKAGE_NAME}",
        path.display()
    );
    Ok(())
}

fn installed_version_from_command(program: &Path, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => parse_installed_version(output.stdout),
        _ => "unknown".to_string(),
    }
}

fn parse_installed_version(stdout: Vec<u8>) -> String {
    let version = String::from_utf8_lossy(&stdout).trim().to_string();
    if version.is_empty() {
        "unknown".to_string()
    } else {
        version
    }
}

fn parse_pacman_installed_version(stdout: Vec<u8>) -> String {
    let text = String::from_utf8_lossy(&stdout);
    let version = text
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    if version.is_empty() {
        "unknown".to_string()
    } else {
        version
    }
}

fn ensure_upgrade_path(path: &Path) -> Result<()> {
    let installed = installed_package_version();
    if installed == "unknown" {
        return Ok(());
    }

    let candidate = deb_package_version(path)?;
    anyhow::ensure!(
        is_version_newer(&candidate, &installed)?,
        "Refusing to install non-newer package version {candidate} over installed version {installed}"
    );
    Ok(())
}

fn ensure_upgrade_path_pacman(path: &Path) -> Result<()> {
    let installed = installed_pacman_version();
    if installed == "unknown" {
        return Ok(());
    }

    let candidate = pacman_package_version(path)?;
    anyhow::ensure!(
        is_version_newer_pacman(&candidate, &installed)?,
        "Refusing to install non-newer package version {candidate} over installed version {installed}"
    );
    Ok(())
}

fn ensure_upgrade_path_rpm(path: &Path) -> Result<()> {
    let installed = installed_rpm_version();
    if installed == "unknown" {
        return Ok(());
    }

    let candidate = rpm_package_version(path)?;
    anyhow::ensure!(
        generated_package_version_is_newer(&candidate, &installed),
        "Refusing to install non-newer package version {candidate} over installed version {installed}"
    );
    Ok(())
}

fn apt_install_command(path: &Path) -> Result<Command> {
    install_command_in_parent(&program_path(APT_CANDIDATES, "apt"), path)
}

fn dpkg_install_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(DPKG_CANDIDATES, "dpkg"));
    command.arg("-i").arg("--").arg(path.as_os_str());
    command
}

fn dnf_install_command(path: &Path) -> Result<Command> {
    install_command_in_parent(&program_path(DNF_CANDIDATES, "dnf"), path)
}

fn zypper_install_command(path: &Path) -> Result<Command> {
    let program = program_path(ZYPPER_CANDIDATES, "zypper");
    let parent = path
        .parent()
        .with_context(|| "zypper package path has no parent directory")?;
    let file_name = path
        .file_name()
        .with_context(|| "zypper package path has no file name")?
        .to_string_lossy()
        .into_owned();

    let mut command = Command::new(program);
    command
        .current_dir(parent)
        .args(["--non-interactive", "--no-gpg-checks", "install", "-y"])
        .arg(format!("./{file_name}"));
    Ok(command)
}

fn install_command_in_parent(program: &Path, path: &Path) -> Result<Command> {
    let program_name = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("package manager");
    let parent = path
        .parent()
        .with_context(|| format!("{program_name} package path has no parent directory"))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("{program_name} package path has no file name"))?
        .to_string_lossy()
        .into_owned();

    let mut command = Command::new(program);
    command
        .current_dir(parent)
        .arg("install")
        .arg("-y")
        .arg(format!("./{file_name}"));
    Ok(command)
}

fn rpm_install_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(RPM_CANDIDATES, "rpm"));
    command.args(["-Uvh", "--"]).arg(path.as_os_str());
    command
}

fn pacman_install_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(PACMAN_CANDIDATES, "pacman"));
    command
        .args(["-U", "--noconfirm", "--"])
        .arg(path.as_os_str());
    command
}

fn updater_binary_for_privileged_install(current_exe: &Path) -> PathBuf {
    let installed = PathBuf::from(INSTALLED_UPDATER_BINARY);
    if installed.is_file() {
        installed
    } else {
        current_exe.to_path_buf()
    }
}

fn deb_package_name(path: &Path) -> Result<String> {
    let output = dpkg_deb_field_command(path, "Package")
        .output()
        .context("Failed to inspect Debian package metadata")?;

    package_metadata_field(output, "dpkg-deb", "package name", path)
}

fn deb_package_version(path: &Path) -> Result<String> {
    let output = dpkg_deb_field_command(path, "Version")
        .output()
        .context("Failed to inspect Debian package metadata")?;

    anyhow::ensure!(
        output.status.success(),
        "dpkg-deb could not read the package version from {}",
        path.display()
    );

    let version = String::from_utf8(output.stdout)
        .context("dpkg-deb returned a non-UTF8 package version")?
        .trim()
        .to_string();
    anyhow::ensure!(
        !version.is_empty(),
        "dpkg-deb returned an empty package version for {}",
        path.display()
    );
    Ok(version)
}

fn rpm_package_name(path: &Path) -> Result<String> {
    let output = rpm_query_command(path, "%{NAME}")
        .output()
        .context("Failed to inspect RPM package metadata")?;

    package_metadata_field(output, "rpm", "package name", path)
}

fn rpm_package_version(path: &Path) -> Result<String> {
    let output = rpm_query_command(path, "%{VERSION}-%{RELEASE}")
        .output()
        .context("Failed to inspect RPM package metadata")?;

    anyhow::ensure!(
        output.status.success(),
        "rpm could not read the package version from {}",
        path.display()
    );

    let version = String::from_utf8(output.stdout)
        .context("rpm returned a non-UTF8 package version")?
        .trim()
        .to_string();
    anyhow::ensure!(
        !version.is_empty(),
        "rpm returned an empty package version for {}",
        path.display()
    );
    Ok(version)
}

fn pacman_package_name(path: &Path) -> Result<String> {
    let output = pacman_query_name_command(path)
        .output()
        .context("Failed to inspect pacman package metadata")?;

    package_metadata_field(output, "pacman", "package name", path)
}

fn dpkg_deb_field_command(path: &Path, field: &str) -> Command {
    let mut command = Command::new(program_path(DPKG_DEB_CANDIDATES, "dpkg-deb"));
    command.arg("-f").arg("--").arg(path).arg(field);
    command
}

fn rpm_query_command(path: &Path, queryformat: &str) -> Command {
    let mut command = Command::new(program_path(RPM_CANDIDATES, "rpm"));
    command
        .arg("-qp")
        .arg("--queryformat")
        .arg(queryformat)
        .arg("--")
        .arg(path);
    command
}

fn pacman_query_name_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(PACMAN_CANDIDATES, "pacman"));
    command.args(["-Qqp", "--"]).arg(path);
    command
}

fn package_metadata_field(
    output: std::process::Output,
    program: &str,
    field: &str,
    path: &Path,
) -> Result<String> {
    anyhow::ensure!(
        output.status.success(),
        "{program} could not read the {field} from {}",
        path.display()
    );

    let value = String::from_utf8(output.stdout)
        .with_context(|| format!("{program} returned a non-UTF8 {field}"))?
        .trim()
        .to_string();
    anyhow::ensure!(
        !value.is_empty(),
        "{program} returned an empty {field} for {}",
        path.display()
    );
    Ok(value)
}

fn is_version_newer(candidate: &str, installed: &str) -> Result<bool> {
    let status = Command::new(program_path(DPKG_CANDIDATES, "dpkg"))
        .args(["--compare-versions", candidate, "gt", installed])
        .status()
        .context("Failed to compare Debian package versions")?;
    Ok(status.success())
}

fn pacman_package_version(path: &Path) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Package path has no file name")?;

    let stripped = strip_pacman_package_suffix(file_name)
        .with_context(|| format!("Not a valid pacman package filename: {file_name}"))?;
    let prefix = format!("{PACKAGE_NAME}-");
    let without_name = stripped
        .strip_prefix(&prefix)
        .with_context(|| format!("Pacman package filename does not start with {prefix}"))?;
    let (version_release, _arch) = without_name
        .rsplit_once('-')
        .context("Pacman package filename is missing an architecture suffix")?;
    anyhow::ensure!(
        !version_release.is_empty(),
        "Could not parse package version from {file_name}"
    );
    Ok(version_release.to_string())
}

fn ensure_gentoo_package(path: &Path) -> Result<()> {
    let package_name = gentoo_artifact_metadata_field(path, "package-name")?;
    anyhow::ensure!(
        package_name == GENTOO_PACKAGE_NAME,
        "Refusing to install Gentoo package {package_name} from {}; expected {GENTOO_PACKAGE_NAME}",
        path.display()
    );
    let category = gentoo_artifact_metadata_field(path, "category")?;
    anyhow::ensure!(
        category == GENTOO_CATEGORY,
        "Refusing to install Gentoo category {category} from {}; expected {GENTOO_CATEGORY}",
        path.display()
    );
    let version = gentoo_package_version(path)?;
    anyhow::ensure!(
        parse_generated_package_version(&version).is_some(),
        "Gentoo package version {version} from {} is not a generated Codex package version",
        path.display()
    );
    let distfile = gentoo_artifact_metadata_field(path, "distfile")?;
    anyhow::ensure!(
        distfile == format!("{GENTOO_PACKAGE_NAME}-{version}.tar.zst"),
        "Gentoo distfile {distfile} from {} does not match package version {version}",
        path.display()
    );
    Ok(())
}

fn gentoo_package_version(path: &Path) -> Result<String> {
    gentoo_artifact_metadata_field(path, "package-version")
}

fn gentoo_artifact_metadata_field(path: &Path, field: &str) -> Result<String> {
    let metadata_path = gentoo_artifact_entry(path, &format!("metadata/{field}"))?;
    let output = Command::new(program_path(TAR_CANDIDATES, "tar"))
        .args(["-I", "zstd", "-xOf"])
        .arg(path)
        .arg(&metadata_path)
        .output()
        .with_context(|| {
            format!(
                "Failed to read Gentoo artifact metadata from {}",
                path.display()
            )
        })?;
    anyhow::ensure!(
        output.status.success(),
        "tar could not read {metadata_path} from {}",
        path.display()
    );
    let value = String::from_utf8(output.stdout)
        .with_context(|| format!("Gentoo artifact metadata {field} is not UTF-8"))?
        .trim()
        .to_string();
    anyhow::ensure!(
        !value.is_empty(),
        "Gentoo artifact metadata {field} is empty in {}",
        path.display()
    );
    Ok(value)
}

fn gentoo_artifact_entry(path: &Path, suffix: &str) -> Result<String> {
    let output = Command::new(program_path(TAR_CANDIDATES, "tar"))
        .args(["-I", "zstd", "-tf"])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to inspect Gentoo artifact {}", path.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "tar could not list Gentoo artifact {}",
        path.display()
    );
    let entries = String::from_utf8(output.stdout)
        .context("tar returned non-UTF8 Gentoo artifact entries")?;
    entries
        .lines()
        .find(|entry| entry == &suffix || entry.ends_with(&format!("/{suffix}")))
        .map(str::to_string)
        .with_context(|| format!("Gentoo artifact {} is missing {suffix}", path.display()))
}

pub(crate) fn merge_gentoo_package(path: &Path) -> Result<()> {
    let extract_dir = create_private_temp_dir()?;
    let result = (|| {
        let mut extract = tar_extract_command(path, &extract_dir);
        run_install(&mut extract).context("Failed to unpack Gentoo overlay artifact")?;
        let artifact_root = gentoo_artifact_root(&extract_dir)?;
        install_gentoo_artifact_root(&artifact_root)
    })();
    let _ = fs::remove_dir_all(&extract_dir);
    result
}

fn tar_extract_command(path: &Path, destination: &Path) -> Command {
    let mut command = Command::new(program_path(TAR_CANDIDATES, "tar"));
    command
        .args(["-I", "zstd", "-xf"])
        .arg(path)
        .arg("-C")
        .arg(destination);
    command
}

fn gentoo_artifact_root(extract_dir: &Path) -> Result<PathBuf> {
    if extract_dir.join("metadata/package-name").is_file() {
        return Ok(extract_dir.to_path_buf());
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(extract_dir)
        .with_context(|| format!("Failed to read {}", extract_dir.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("metadata/package-name").is_file() {
            candidates.push(entry.path());
        }
    }
    anyhow::ensure!(
        candidates.len() == 1,
        "Gentoo artifact must contain exactly one bundle root with metadata/package-name"
    );
    Ok(candidates.remove(0))
}

fn install_gentoo_artifact_root(root: &Path) -> Result<()> {
    let package_name = read_trimmed(root.join("metadata/package-name"))?;
    anyhow::ensure!(
        package_name == GENTOO_PACKAGE_NAME,
        "Gentoo artifact package is {package_name}; expected {GENTOO_PACKAGE_NAME}"
    );
    let category = read_trimmed(root.join("metadata/category"))?;
    anyhow::ensure!(
        category == GENTOO_CATEGORY,
        "Gentoo artifact category is {category}; expected {GENTOO_CATEGORY}"
    );
    let repo_name = read_trimmed(root.join("metadata/repo-name"))?;
    anyhow::ensure!(
        repo_name == GENTOO_REPO_NAME,
        "Gentoo artifact repo is {repo_name}; expected {GENTOO_REPO_NAME}"
    );
    let version = read_trimmed(root.join("metadata/package-version"))?;
    let atom = read_trimmed(root.join("metadata/atom"))?;
    let distfile = read_trimmed(root.join("metadata/distfile"))?;
    anyhow::ensure!(
        atom == format!("={GENTOO_CATEGORY}/{GENTOO_PACKAGE_NAME}-{version}"),
        "Gentoo artifact atom {atom} does not match version {version}"
    );

    let distdir = gentoo_distdir();
    fs::create_dir_all(&distdir)
        .with_context(|| format!("Failed to create Portage distdir {}", distdir.display()))?;
    copy_file(
        &root.join("distfiles").join(&distfile),
        &distdir.join(&distfile),
    )?;
    set_world_readable_file_permissions(&distdir.join(&distfile))?;

    let repo_root = Path::new("/var/db/repos").join(&repo_name);
    copy_gentoo_overlay(&root.join("overlay"), &repo_root)?;
    write_gentoo_repos_conf(&repo_name, &repo_root)?;
    refresh_gentoo_metadata(&repo_name);

    let mut command = gentoo_emerge_command(&atom);
    run_install(&mut command).with_context(|| format!("emerge {atom} failed"))
}

fn read_trimmed(path: PathBuf) -> Result<String> {
    let value =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let value = value.trim().to_string();
    anyhow::ensure!(!value.is_empty(), "{} is empty", path.display());
    Ok(value)
}

fn copy_gentoo_overlay(source: &Path, destination: &Path) -> Result<()> {
    anyhow::ensure!(
        source.join("profiles/repo_name").is_file(),
        "Gentoo overlay source is missing profiles/repo_name: {}",
        source.display()
    );
    if destination.exists() {
        fs::remove_dir_all(destination)
            .with_context(|| format!("Failed to replace {}", destination.display()))?;
    }
    copy_dir_recursive(source, destination)
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Failed to create {}", destination.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("Failed to read {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .with_context(|| format!("{} has no parent directory", destination.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    fs::copy(source, destination).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

#[cfg(unix)]
fn set_world_readable_file_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_world_readable_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn write_gentoo_repos_conf(repo_name: &str, repo_root: &Path) -> Result<()> {
    let repos_conf_dir = Path::new("/etc/portage/repos.conf");
    fs::create_dir_all(repos_conf_dir)
        .with_context(|| format!("Failed to create {}", repos_conf_dir.display()))?;
    let repos_conf = repos_conf_dir.join(format!("{repo_name}.conf"));
    let content = format!(
        "[{repo_name}]\nlocation = {}\nmasters = gentoo\nauto-sync = no\n",
        repo_root.display()
    );
    fs::write(&repos_conf, content)
        .with_context(|| format!("Failed to write {}", repos_conf.display()))
}

fn gentoo_distdir() -> PathBuf {
    match Command::new(program_path(PORTAGEQ_CANDIDATES, "portageq"))
        .args(["envvar", "DISTDIR"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !value.is_empty() {
                return PathBuf::from(value);
            }
        }
        _ => {}
    }
    PathBuf::from("/var/cache/distfiles")
}

fn refresh_gentoo_metadata(repo_name: &str) {
    let egencache = program_path(&["/usr/bin/egencache", "/bin/egencache"], "egencache");
    let _ = Command::new(egencache)
        .args(["--repo", repo_name, "--update"])
        .status();
}

fn gentoo_emerge_command(atom: &str) -> Command {
    let mut command = Command::new(program_path(EMERGE_CANDIDATES, "emerge"));
    command.args(["--oneshot", "--verbose", atom]);
    command
}

fn ensure_upgrade_path_gentoo(path: &Path) -> Result<()> {
    let installed = installed_gentoo_version();
    if installed == "unknown" {
        return Ok(());
    }

    let candidate = gentoo_package_version(path)?;
    anyhow::ensure!(
        generated_package_version_is_newer(&candidate, &installed),
        "Refusing to install non-newer Gentoo package version {candidate} over installed version {installed}"
    );
    Ok(())
}

fn is_version_newer_pacman(candidate: &str, installed: &str) -> Result<bool> {
    let output = Command::new(program_path(VERCMP_CANDIDATES, "vercmp"))
        .args([candidate, installed])
        .output()
        .context("Failed to compare pacman package versions")?;
    anyhow::ensure!(
        output.status.success(),
        "vercmp exited with status {}",
        output.status
    );

    let comparison = String::from_utf8(output.stdout)
        .context("vercmp returned a non-UTF8 response")?
        .trim()
        .parse::<i32>()
        .context("vercmp returned an invalid comparison value")?;
    Ok(comparison > 0)
}

fn generated_package_version_is_newer(candidate: &str, installed: &str) -> bool {
    matches!(
        compare_generated_package_versions(candidate, installed),
        Some(std::cmp::Ordering::Greater)
    )
}

fn compare_generated_package_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let left = parse_generated_package_version(left)?;
    let right = parse_generated_package_version(right)?;
    Some(left.cmp(&right))
}

fn parse_generated_package_version(version: &str) -> Option<Vec<u32>> {
    let without_metadata = version
        .split_once('+')
        .map(|(prefix, _)| prefix)
        .unwrap_or(version);
    let base = without_metadata
        .split_once('-')
        .map(|(prefix, _)| prefix)
        .unwrap_or(without_metadata);
    let mut parts = Vec::new();

    for segment in base.split('.') {
        parts.push(segment.parse().ok()?);
    }

    if parts.len() < 3 || !(2000..=2100).contains(&parts[0]) {
        return None;
    }

    Some(parts)
}

fn strip_pacman_package_suffix(file_name: &str) -> Option<&str> {
    let lower = file_name.to_ascii_lowercase();
    PACMAN_PACKAGE_SUFFIXES.iter().find_map(|suffix| {
        lower
            .strip_suffix(suffix)
            .map(|_| &file_name[..file_name.len() - suffix.len()])
    })
}

fn is_pacman_package_file_name(file_name: &str) -> bool {
    strip_pacman_package_suffix(file_name).is_some()
}

fn is_gentoo_package_file_name(file_name: &str) -> bool {
    file_name
        .to_ascii_lowercase()
        .ends_with(GENTOO_PACKAGE_SUFFIX)
}

fn program_exists(candidates: &[&str], fallback: &str) -> bool {
    candidates.iter().any(|path| Path::new(path).is_file()) || command_exists(fallback)
}

fn program_path(candidates: &[&str], fallback: &str) -> PathBuf {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(fallback))
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|entry| {
                let candidate: PathBuf = entry.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn builds_pkexec_command_for_privileged_deb_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.deb"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "--disable-internal-agent",
                "/usr/bin/codex-update-manager",
                "install-deb",
                "--path",
                "/tmp/update.deb"
            ]
        );
    }

    #[test]
    fn builds_pkexec_command_for_privileged_rpm_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.rpm"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "--disable-internal-agent",
                "/usr/bin/codex-update-manager",
                "install-rpm",
                "--path",
                "/tmp/update.rpm"
            ]
        );
    }

    #[test]
    fn prefers_installed_updater_path_for_pkexec() {
        let selected =
            updater_binary_for_privileged_install(Path::new("/tmp/codex-update-manager-old"));
        let expected = if Path::new("/usr/bin/codex-update-manager").is_file() {
            PathBuf::from("/usr/bin/codex-update-manager")
        } else {
            PathBuf::from("/tmp/codex-update-manager-old")
        };
        assert_eq!(selected, expected);
    }

    #[test]
    fn builds_local_apt_install_command() -> Result<()> {
        let command = apt_install_command(Path::new("/tmp/build/codex.deb"))?;
        assert!(command.get_program().to_string_lossy().ends_with("apt"));
        assert_eq!(
            command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["install", "-y", "./codex.deb"]
        );
        Ok(())
    }

    #[test]
    fn builds_local_dnf_install_command() -> Result<()> {
        let command = dnf_install_command(Path::new("/tmp/build/codex.rpm"))?;
        let program = command.get_program().to_string_lossy();
        assert!(program.ends_with("dnf") || program.ends_with("dnf5"));
        assert_eq!(
            command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["install", "-y", "./codex.rpm"]
        );
        Ok(())
    }

    #[test]
    fn builds_local_zypper_install_command() -> Result<()> {
        let command = zypper_install_command(Path::new("/tmp/build/codex.rpm"))?;
        assert!(command.get_program().to_string_lossy().ends_with("zypper"));
        assert_eq!(
            command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec![
                "--non-interactive",
                "--no-gpg-checks",
                "install",
                "-y",
                "./codex.rpm"
            ]
        );
        Ok(())
    }

    #[test]
    fn direct_install_commands_stop_option_parsing() {
        assert_eq!(
            command_args(dpkg_install_command(Path::new("-evil.deb"))),
            vec!["-i", "--", "-evil.deb"]
        );
        assert_eq!(
            command_args(rpm_install_command(Path::new("-evil.rpm"))),
            vec!["-Uvh", "--", "-evil.rpm"]
        );
        assert_eq!(
            command_args(pacman_install_command(Path::new("-evil.pkg.tar.zst"))),
            vec!["-U", "--noconfirm", "--", "-evil.pkg.tar.zst"]
        );
    }

    #[test]
    fn metadata_commands_stop_option_parsing_before_package_path() {
        assert_eq!(
            command_args(dpkg_deb_field_command(Path::new("-evil.deb"), "Package")),
            vec!["-f", "--", "-evil.deb", "Package"]
        );
        assert_eq!(
            command_args(rpm_query_command(Path::new("-evil.rpm"), "%{NAME}")),
            vec!["-qp", "--queryformat", "%{NAME}", "--", "-evil.rpm"]
        );
        assert_eq!(
            command_args(pacman_query_name_command(Path::new("-evil.pkg.tar.zst"))),
            vec!["-Qqp", "--", "-evil.pkg.tar.zst"]
        );
    }

    #[test]
    fn stable_file_name_uses_safe_names_for_deb_and_rpm() -> Result<()> {
        assert_eq!(
            stable_file_name(PackageKind::Deb, Path::new("-evil.deb"))?,
            "codex-desktop.deb"
        );
        assert_eq!(
            stable_file_name(PackageKind::Rpm, Path::new("-evil.rpm"))?,
            "codex-desktop.rpm"
        );
        assert_eq!(
            stable_file_name(PackageKind::Gentoo, Path::new("-evil.gentoo.tar.zst"))?,
            "codex-desktop.gentoo.tar.zst"
        );
        assert_eq!(
            stable_file_name(
                PackageKind::Pacman,
                Path::new("/tmp/codex-desktop-2026.03.30-1-x86_64.pkg.tar.zst")
            )?,
            "codex-desktop-2026.03.30-1-x86_64.pkg.tar.zst"
        );
        Ok(())
    }

    fn command_args(command: Command) -> Vec<String> {
        command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn package_kind_from_path_detects_rpm() {
        assert_eq!(
            PackageKind::from_path(Path::new("/tmp/codex.rpm")),
            PackageKind::Rpm
        );
    }

    #[test]
    fn package_kind_from_path_detects_deb() {
        assert_eq!(
            PackageKind::from_path(Path::new("/tmp/codex.deb")),
            PackageKind::Deb
        );
    }

    #[test]
    fn package_kind_from_path_detects_pacman_zst() {
        assert_eq!(
            PackageKind::from_path(Path::new(
                "/tmp/codex-desktop-2026.03.30-1-x86_64.pkg.tar.zst"
            )),
            PackageKind::Pacman
        );
    }

    #[test]
    fn package_kind_from_path_detects_gentoo_before_generic_tar_zst() {
        assert_eq!(
            PackageKind::from_path(Path::new(
                "/tmp/codex-desktop-2026.03.30-amd64.gentoo.tar.zst"
            )),
            PackageKind::Gentoo
        );
    }

    #[test]
    fn package_kind_from_path_detects_pacman_xz() {
        assert_eq!(
            PackageKind::from_path(Path::new(
                "/tmp/codex-desktop-2026.03.30-1-x86_64.pkg.tar.xz"
            )),
            PackageKind::Pacman
        );
    }

    #[test]
    fn detection_prefers_arch_os_release_even_if_rpm_command_exists() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_pacman: true,
                has_rpm: true,
                pacman_installed: true,
                os_release: Some(("arch".to_string(), "".to_string())),
                ..Default::default()
            }),
            PackageKind::Pacman
        );
    }

    #[test]
    fn detection_prefers_fedora_os_release_even_if_deb_package_is_installed() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_dpkg: true,
                has_rpm: true,
                deb_installed: true,
                os_release: Some(("fedora".to_string(), "rhel".to_string())),
                ..Default::default()
            }),
            PackageKind::Rpm
        );
    }

    #[test]
    fn detection_prefers_gentoo_os_release() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_pacman: true,
                has_emerge: true,
                has_dpkg: true,
                has_rpm: true,
                os_release: Some(("gentoo".to_string(), "".to_string())),
                ..Default::default()
            }),
            PackageKind::Gentoo
        );
    }

    #[test]
    fn detection_uses_installed_gentoo_package_before_tool_fallbacks() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_dpkg: true,
                has_rpm: true,
                gentoo_installed: true,
                ..Default::default()
            }),
            PackageKind::Gentoo
        );
    }

    #[test]
    fn detection_uses_arch_os_release_when_nothing_is_installed() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_pacman: true,
                has_rpm: true,
                os_release: Some(("arch".to_string(), "".to_string())),
                ..Default::default()
            }),
            PackageKind::Pacman
        );
    }

    #[test]
    fn detection_uses_debian_os_release_before_rpm_command_presence() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_dpkg: true,
                has_rpm: true,
                os_release: Some(("ubuntu".to_string(), "debian".to_string())),
                ..Default::default()
            }),
            PackageKind::Deb
        );
    }

    #[test]
    fn detection_uses_rpm_os_release_before_pacman_command_presence() {
        assert_eq!(
            detect_package_kind(PackageDetection {
                has_pacman: true,
                has_rpm: true,
                os_release: Some(("fedora".to_string(), "rhel".to_string())),
                ..Default::default()
            }),
            PackageKind::Rpm
        );
    }

    #[test]
    fn trims_quoted_os_release_values() {
        assert_eq!(trim_os_release_value("\"arch\""), "arch");
        assert_eq!(trim_os_release_value("'debian ubuntu'"), "debian ubuntu");
    }

    #[test]
    fn matches_expected_os_release_tokens() {
        assert!(os_release_matches(&["ubuntu debian", ""], &["debian"]));
        assert!(!os_release_matches(&["ubuntu", ""], &["fedora"]));
    }

    #[test]
    fn builds_pkexec_command_for_privileged_pacman_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.pkg.tar.zst"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "--disable-internal-agent",
                "/usr/bin/codex-update-manager",
                "install-pacman",
                "--path",
                "/tmp/update.pkg.tar.zst"
            ]
        );
    }

    #[test]
    fn builds_pkexec_command_for_privileged_gentoo_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.gentoo.tar.zst"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "--disable-internal-agent",
                "/usr/bin/codex-update-manager",
                "install-gentoo",
                "--path",
                "/tmp/update.gentoo.tar.zst"
            ]
        );
    }

    #[test]
    fn compares_debian_versions_using_dpkg_rules() -> Result<()> {
        if !program_exists(DPKG_CANDIDATES, "dpkg") {
            return Ok(());
        }

        assert!(is_version_newer(
            "2026.03.24.220000+88f07cd3",
            "2026.03.24.120000+afed8a8e"
        )?);
        assert!(!is_version_newer(
            "2026.03.24.120000+88f07cd3",
            "2026.03.24.120000+afed8a8e"
        )?);
        Ok(())
    }

    #[test]
    fn compares_generated_package_versions_by_timestamp() {
        assert!(generated_package_version_is_newer(
            "2026.04.28.140000-abcdef12.fc43",
            "2026.04.28.082247-12345678.fc43"
        ));
        assert!(!generated_package_version_is_newer(
            "2026.04.28.082247-12345678.fc43",
            "2026.04.28.140000-abcdef12.fc43"
        ));
        assert!(!generated_package_version_is_newer(
            "2026.04.28.140000-abcdef12.fc43",
            "2026.04.28.140000-abcdef12.fc43"
        ));
    }

    #[test]
    fn generated_package_version_comparison_rejects_non_generated_versions() {
        assert_eq!(
            compare_generated_package_versions("0.4.2", "2026.04.28.082247-12345678.fc43"),
            None
        );
        assert!(!generated_package_version_is_newer(
            "0.4.2",
            "2026.04.28.082247-12345678.fc43"
        ));
    }

    #[test]
    fn install_commands_require_a_file_name() {
        let deb_error = apt_install_command(Path::new("/")).expect_err("root is not a package");
        let rpm_error = dnf_install_command(Path::new("/")).expect_err("root is not a package");
        let zypper_error =
            zypper_install_command(Path::new("/")).expect_err("root is not a package");

        assert!(deb_error.to_string().contains("apt package path has no"));
        assert!(rpm_error.to_string().contains("dnf package path has no"));
        assert!(zypper_error
            .to_string()
            .contains("zypper package path has no"));
    }

    #[test]
    fn empty_installed_version_output_is_reported_as_unknown() {
        assert_eq!(parse_installed_version(Vec::new()), "unknown");
    }

    #[test]
    fn parses_pacman_installed_version_output() {
        assert_eq!(
            parse_pacman_installed_version(b"codex-desktop 2026.04.02.120000-1\n".to_vec()),
            "2026.04.02.120000-1"
        );
    }

    #[test]
    fn parses_gentoo_installed_version_from_vdb() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let category = temp.path().join(GENTOO_CATEGORY);
        std::fs::create_dir_all(category.join("codex-desktop-bin-2026.04.02.120000"))?;
        std::fs::create_dir_all(category.join("codex-desktop-bin-2026.04.03.090000"))?;

        assert_eq!(
            installed_gentoo_version_from_vdb(temp.path()),
            "2026.04.03.090000"
        );
        Ok(())
    }

    #[test]
    fn parses_pacman_package_version_from_filename() -> Result<()> {
        assert_eq!(
            pacman_package_version(Path::new(
                "/tmp/codex-desktop-2026.04.02.120000-1-x86_64.pkg.tar.zst"
            ))?,
            "2026.04.02.120000-1"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn resolves_pacman_latest_symlink_to_versioned_package_identity() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let package_name = "codex-desktop-2026.04.02.120000-1-x86_64.pkg.tar.zst";
        let package_path = temp.path().join(package_name);
        let latest_path = temp.path().join("codex-desktop-latest.pkg.tar.zst");
        std::fs::write(&package_path, b"pkg")?;
        std::os::unix::fs::symlink(package_name, &latest_path)?;

        let identity_path = package_identity_path(&latest_path)?;

        assert_eq!(
            identity_path.file_name().and_then(|name| name.to_str()),
            Some(package_name)
        );
        assert_eq!(
            stable_file_name(PackageKind::Pacman, &identity_path)?,
            package_name
        );
        assert_eq!(
            pacman_package_version(&identity_path)?,
            "2026.04.02.120000-1"
        );
        Ok(())
    }

    #[test]
    fn rejects_mismatched_package_name() {
        let error = ensure_package_name("not-codex", Path::new("/tmp/not-codex.deb"))
            .expect_err("foreign package names must be rejected");

        assert!(error.to_string().contains("expected codex-desktop"));
    }

    #[test]
    fn accepts_codex_package_name() -> Result<()> {
        ensure_package_name("codex-desktop", Path::new("/tmp/codex-desktop.deb"))
    }

    #[test]
    fn rejects_non_codex_pacman_package_filename() {
        let error = ensure_codex_package(Path::new(
            "/tmp/not-codex-2026.04.02.120000-1-x86_64.pkg.tar.zst",
        ))
        .expect_err("foreign pacman packages must be rejected");

        assert!(error.to_string().contains("codex-desktop-"));
    }
}
