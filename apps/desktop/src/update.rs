//! Updating the app from the server it is connected to.
//!
//! The server says which version it is and where the archive of the Mac app
//! for that version is (`GET /api/v1/downloads`). A newer version is
//! downloaded into a private folder of the app's data directory, checked,
//! unpacked, checked again and swapped into the place of the running bundle.
//!
//! The server is self-hosted and may have been taken over, so nothing it
//! says is trusted further than this: the archive must have the size and
//! SHA-256 it advertised, and the app inside must be signed with a Developer
//! ID of the team that signed the running app, carry this app's identifier,
//! be accepted by Gatekeeper, and be the version that was advertised and a
//! later one than the one running. Anything else is refused and the
//! installed app is left as it is.
//!
//! This file is the work itself, with no window in it; `update_view.rs`
//! shows it.
use crate::fetch::{self, Request};
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock, atomic::AtomicU64},
};

/// The identifier every build of this app is signed with.
const IDENTIFIER: &str = "dev.cereal.desktop";
/// The Developer ID team that signs releases. A running app with a Developer
/// ID signature names its own team; this is what is required when it cannot
/// be read.
const TEAM: &str = "LZWN2FF7YB";
const FILES_PATH: &str = "/api/v1/downloads/files/";
const MANIFEST_LIMIT: usize = 65_536;
/// No archive of this app is anywhere near this large.
const ARCHIVE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
/// What the release script names the app inside the archive.
const ARCHIVE_APP: &str = "Cereal.app";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The app is not set up to use a server.
    NotConnected,
    Unreachable,
    /// The server's answer is not one this app can act on.
    Manifest,
    /// The server is a later version, and has no archive of it for this Mac.
    NoBuild,
    Download,
    Unverified,
    /// A debug or ad hoc signed build, or one that is not in a bundle.
    CannotUpdateItself,
    /// Running translocated or from a volume that cannot be written.
    MoveToApplications,
    /// The new version needs a later macOS than this Mac has.
    NeedsSystem(String),
    Install,
}

impl Problem {
    pub fn message(&self) -> String {
        match self {
            Problem::NotConnected => "Connect to a server to check for updates.".into(),
            Problem::Unreachable => "Could not reach this server to check for updates.".into(),
            Problem::Manifest => "This server's update information could not be read.".into(),
            Problem::NoBuild => "No build has been published on this server.".into(),
            Problem::Download => "The download failed. Try again.".into(),
            Problem::Unverified => {
                "The update could not be verified and was not installed".into()
            }
            Problem::CannotUpdateItself => "This build cannot update itself".into(),
            Problem::MoveToApplications => {
                "Move Cereal to the Applications folder to update".into()
            }
            Problem::NeedsSystem(version) => format!("This update needs macOS {version} or later"),
            Problem::Install => {
                "The update could not be installed. Cereal was left as it was.".into()
            }
        }
    }
}

// Versions.

/// The numbers of a dotted version, without a pre-release or build suffix.
fn version_numbers(version: &str) -> Option<Vec<u64>> {
    let core = version.split(['-', '+']).next()?;
    let numbers: Vec<u64> = core
        .split('.')
        .map(|part| {
            (!part.is_empty() && part.len() <= 9 && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;
    (!numbers.is_empty() && numbers.len() <= 4).then_some(numbers)
}

/// Whether `candidate` is a strictly later version than `current`, number by
/// number (0.1.10 is later than 0.1.9; 1.0 and 1.0.0 are the same). `None`
/// when either is not a dotted version.
pub fn newer(candidate: &str, current: &str) -> Option<bool> {
    let (mut candidate, mut current) = (version_numbers(candidate)?, version_numbers(current)?);
    let length = candidate.len().max(current.len());
    candidate.resize(length, 0);
    current.resize(length, 0);
    Some(candidate > current)
}

/// The bundle an executable is in: `<name>.app/Contents/MacOS/<executable>`.
fn bundle_of(executable: &Path) -> Option<PathBuf> {
    let contents = executable.parent()?.parent()?;
    let bundle = contents.parent()?;
    (executable.parent()?.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

/// The bundle this process was started from, if it was started from one.
fn running_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let bundle = bundle_of(&executable)?;
    bundle.join("Contents/Info.plist").is_file().then_some(bundle)
}

/// One string of a bundle's Info.plist.
fn bundle_string(bundle: &Path, key: &str) -> Option<String> {
    let path = bundle.join("Contents/Info.plist");
    let bytes = std::fs::read(&path).ok()?;
    if let Some(value) = std::str::from_utf8(&bytes)
        .ok()
        .filter(|text| text.trim_start().starts_with("<?xml"))
        .and_then(|text| xml_plist_string(text, key))
    {
        return Some(value);
    }
    // Any other form of property list is read by the system's own tool.
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(&path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let value = String::from_utf8(output.stdout).ok()?;
    (output.status.success() && !value.trim().is_empty()).then(|| value.trim().to_owned())
}

/// `<key>name</key><string>value</string>` of an XML property list.
fn xml_plist_string(text: &str, key: &str) -> Option<String> {
    let after = text.split_once(&format!("<key>{key}</key>"))?.1.trim_start();
    let value = after.strip_prefix("<string>")?.split_once("</string>")?.0;
    (!value.is_empty() && !value.contains('<')).then(|| value.to_owned())
}

/// This app's version: the one its bundle carries, so a build can be
/// packaged under another number, or the one it was compiled with when it
/// is not running from a bundle.
pub fn current_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        running_bundle()
            .and_then(|bundle| bundle_string(&bundle, "CFBundleShortVersionString"))
            .filter(|version| version_numbers(version).is_some())
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
    })
}

// What the server offers.

/// The archive of a version: where it is on the server and what it must be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    /// A path on the server that offered it, never another address.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    /// `None`: no archive of this version has been published for this Mac.
    pub update: Option<Offer>,
}

/// The processor this build is for, as the server names it.
fn own_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

fn file_name_allowed(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_alphanumeric())
        && name.len() <= 200
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
        && !name.contains("..")
        && name.ends_with(".zip")
}

impl Manifest {
    /// Read the server's answer. Fields this app does not know are ignored.
    /// An archive is accepted only as a file of this server's own downloads:
    /// its address must be the downloads path followed by its file name.
    pub fn read(body: &[u8], architecture: &str) -> Result<Self, Problem> {
        let value: Value = serde_json::from_slice(body).map_err(|_| Problem::Manifest)?;
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .filter(|version| version.len() <= 40 && version_numbers(version).is_some())
            .ok_or(Problem::Manifest)?
            .to_owned();
        let update = match value.get("mac") {
            None | Some(Value::Null) => None,
            Some(mac) => {
                let ours = mac.get("arch").and_then(Value::as_str) == Some(architecture);
                match mac.get("update") {
                    None | Some(Value::Null) => None,
                    Some(_) if !ours => None,
                    Some(update) => Some(Self::offer(update, &version)?),
                }
            }
        };
        Ok(Self { version, update })
    }

    fn offer(update: &Value, version: &str) -> Result<Offer, Problem> {
        let text = |name: &str| update.get(name).and_then(Value::as_str);
        let file = text("file")
            .filter(|file| file_name_allowed(file))
            .ok_or(Problem::Manifest)?;
        let path = text("url")
            .filter(|url| url.strip_prefix(FILES_PATH) == Some(file))
            .ok_or(Problem::Manifest)?;
        let size = update
            .get("size")
            .and_then(Value::as_u64)
            .filter(|size| (1..=ARCHIVE_LIMIT).contains(size))
            .ok_or(Problem::Manifest)?;
        let sha256 = text("sha256")
            .filter(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or(Problem::Manifest)?;
        Ok(Offer {
            version: version.to_owned(),
            path: path.to_owned(),
            size,
            sha256: sha256.to_owned(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    UpToDate,
    Available(Offer),
}

/// What a server's answer means for an app at version `current`. The same
/// or an earlier version is "up to date": the app never goes back.
pub fn evaluate(manifest: Manifest, current: &str) -> Result<Check, Problem> {
    match newer(&manifest.version, current) {
        Some(true) => manifest.update.map(Check::Available).ok_or(Problem::NoBuild),
        Some(false) => Ok(Check::UpToDate),
        None => Err(Problem::Manifest),
    }
}

/// Ask `server` what it offers. No credentials are sent.
pub fn check(server: &str) -> Result<Check, Problem> {
    let address = format!("{server}/api/v1/downloads");
    let reply = Request {
        method: "GET",
        url: &address,
        body: None,
    }
    .send(MANIFEST_LIMIT)
    .map_err(|failure| match failure {
        fetch::Failure::TooLarge => Problem::Manifest,
        _ => Problem::Unreachable,
    })?;
    match reply.status {
        200 => evaluate(Manifest::read(&reply.body, own_architecture())?, current_version()),
        // A server from before it handed out apps.
        404 => Err(Problem::NoBuild),
        _ => Err(Problem::Unreachable),
    }
}

// Downloading and checking.

/// The app's data directory: the one `CEREAL_DATA_DIR` names, or the usual
/// one under Application Support.
fn data_directory() -> Option<PathBuf> {
    match crate::environment::var_os("CEREAL_DATA_DIR").filter(|value| !value.is_empty()) {
        Some(directory) => Some(PathBuf::from(directory)),
        None => Some(
            PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Cereal"),
        ),
    }
}

/// The private folder an update is downloaded and unpacked in.
fn work_directory() -> Option<PathBuf> {
    Some(data_directory()?.join("update"))
}

/// An empty folder at `path` that only this user can read.
fn fresh_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if path.symlink_metadata().is_ok() {
        if path.is_dir() && !path.is_symlink() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether the file at `path` is the archive that was offered: exactly its
/// size, and its SHA-256. A file that stops short is a failed download; any
/// other difference is an archive that cannot be trusted.
pub fn check_archive(path: &Path, offer: &Offer) -> Result<(), Problem> {
    let mut file = std::fs::File::open(path).map_err(|_| Problem::Download)?;
    let size = file.metadata().map_err(|_| Problem::Download)?.len();
    if size < offer.size {
        return Err(Problem::Download);
    }
    if size > offer.size {
        return Err(Problem::Unverified);
    }
    let mut hash = crate::handoff::Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        match file.read(&mut buffer).map_err(|_| Problem::Download)? {
            0 => break,
            count => hash.update(&buffer[..count]),
        }
    }
    if hex(&hash.finish()) == offer.sha256 {
        Ok(())
    } else {
        Err(Problem::Unverified)
    }
}

/// Download the offered archive from `server` into a fresh private folder
/// and check it is what was offered. Every attempt starts again from the
/// beginning. `received` follows the bytes on disk.
pub fn download(server: &str, offer: &Offer, received: &AtomicU64) -> Result<PathBuf, Problem> {
    // The clearing-up at launch is over before the folder is used again.
    if let Some(tidying) = TIDYING.lock().ok().and_then(|mut tidying| tidying.take()) {
        let _ = tidying.join();
    }
    let directory = work_directory().ok_or(Problem::Download)?;
    fresh_private_directory(&directory).map_err(|_| Problem::Download)?;
    let archive = directory.join("update.zip");
    let address = format!("{server}{}", offer.path);
    // The size comes from the manifest; the server need not state one.
    match fetch::download(&address, &archive, offer.size, received) {
        Ok(200) => check_archive(&archive, offer).map(|()| archive),
        Ok(_) => Err(Problem::Download),
        Err(fetch::Failure::TooLarge) => Err(Problem::Unverified),
        Err(_) => Err(Problem::Download),
    }
}

fn succeeds(command: &mut Command) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether an archive holds only what a release holds: entries inside
/// `Cereal.app`, none that climbs out of it, and no symbolic link (a release
/// has none, and one could lead what is unpacked after it somewhere else).
/// `names` is one entry per line; `listing` is the long listing, whose
/// lines start with each entry's kind and permissions.
pub fn archive_entries_allowed(names: &str, listing: &str) -> bool {
    let inside = format!("{ARCHIVE_APP}/");
    let mut count = 0;
    let contained = names.lines().all(|name| {
        count += 1;
        name.starts_with(&inside)
            && !name.contains('\\')
            && !name.split('/').any(|part| part == "..")
    });
    let links = listing
        .lines()
        .any(|line| line.starts_with("lr") || line.starts_with("l-"));
    contained && !links && (1..=50_000).contains(&count)
}

fn archive_listing(archive: &Path, long: bool) -> Option<String> {
    let mut command = Command::new("/usr/bin/zipinfo");
    if !long {
        command.arg("-1");
    }
    let output = command
        .arg(archive)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Unpack the archive beside itself and return the app inside it.
pub fn unpack(archive: &Path) -> Result<PathBuf, Problem> {
    let allowed = match (archive_listing(archive, false), archive_listing(archive, true)) {
        (Some(names), Some(listing)) => archive_entries_allowed(&names, &listing),
        _ => false,
    };
    if !allowed {
        return Err(Problem::Unverified);
    }
    let directory = archive.parent().ok_or(Problem::Unverified)?.join("unpacked");
    fresh_private_directory(&directory).map_err(|_| Problem::Install)?;
    if !succeeds(
        Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(archive)
            .arg(&directory),
    ) {
        return Err(Problem::Unverified);
    }
    let app = directory.join(ARCHIVE_APP);
    let real = app
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
    if real { Ok(app) } else { Err(Problem::Unverified) }
}

fn team_allowed(team: &str) -> bool {
    team.len() == 10
        && team
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

/// A Developer ID signature: Apple's anchor, the Developer ID intermediate
/// and a Developer ID Application leaf.
const DEVELOPER_ID: &str = "anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// The code requirement a new bundle must satisfy: a Developer ID signature
/// of `team` on this app's identifier.
pub fn requirement(team: &str) -> Option<String> {
    team_allowed(team).then(|| {
        format!(
            "anchor apple generic and identifier \"{IDENTIFIER}\" and certificate leaf[subject.OU] = \"{team}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
        )
    })
}

/// The team whose Developer ID signed the bundle at `bundle`, or `None` for
/// a debug, ad hoc or otherwise signed build.
fn developer_id_team(bundle: &Path) -> Option<String> {
    if !succeeds(
        Command::new("/usr/bin/codesign")
            .arg("--verify")
            .arg(format!("-R={DEVELOPER_ID}"))
            .arg(bundle),
    ) {
        return None;
    }
    let details = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=2"])
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .ok()?;
    let team = String::from_utf8_lossy(&details.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier=").map(str::to_owned))
        .filter(|team| team_allowed(team))
        .unwrap_or_else(|| TEAM.to_owned());
    Some(team)
}

/// This Mac's version of macOS.
fn system_version() -> Option<String> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    version_numbers(&version).map(|_| version)
}

/// The version an unpacked bundle must carry: the one that was advertised,
/// and a later one than the one running.
pub fn check_bundle_version(found: Option<&str>, offer: &Offer, current: &str) -> Result<(), Problem> {
    match found {
        Some(found) if found == offer.version && newer(found, current) == Some(true) => Ok(()),
        _ => Err(Problem::Unverified),
    }
}

/// Decide whether the unpacked app may replace the running one. The
/// signature is checked first; only a bundle that passed is read further.
pub fn verify_bundle(app: &Path, offer: &Offer, team: &str) -> Result<(), Problem> {
    let requirement = requirement(team).ok_or(Problem::Unverified)?;
    if !succeeds(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict", "--all-architectures"])
            .arg(format!("-R={requirement}"))
            .arg(app),
    ) {
        return Err(Problem::Unverified);
    }
    if !succeeds(
        Command::new("/usr/sbin/spctl")
            .args(["--assess", "--type", "execute"])
            .arg(app),
    ) {
        return Err(Problem::Unverified);
    }
    check_bundle_version(
        bundle_string(app, "CFBundleShortVersionString").as_deref(),
        offer,
        current_version(),
    )?;
    // A version this Mac cannot open would leave no working app behind.
    if let (Some(needed), Some(system)) = (
        bundle_string(app, "LSMinimumSystemVersion"),
        system_version(),
    ) && newer(&needed, &system) == Some(true)
    {
        return Err(Problem::NeedsSystem(needed));
    }
    Ok(())
}

// Installing.

/// Where the running app is, when it can be replaced there. An app that
/// macOS runs from a translocated copy, or that sits on a volume that cannot
/// be written (a disk image), has to be moved first.
pub fn install_target(
    executable: &Path,
    read_only: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, Problem> {
    let bundle = bundle_of(executable).ok_or(Problem::CannotUpdateItself)?;
    if bundle
        .components()
        .any(|part| part.as_os_str() == "AppTranslocation")
    {
        return Err(Problem::MoveToApplications);
    }
    let parent = bundle.parent().ok_or(Problem::MoveToApplications)?;
    if read_only(parent) {
        return Err(Problem::MoveToApplications);
    }
    Ok(bundle)
}

/// Whether nothing can be created in `directory`.
fn cannot_write(directory: &Path) -> bool {
    let probe = directory.join(format!(".cereal-update-{}", std::process::id()));
    match std::fs::create_dir(&probe) {
        Ok(()) => {
            let _ = std::fs::remove_dir(&probe);
            false
        }
        Err(_) => true,
    }
}

/// The app that would be replaced and the team its replacement must be
/// signed by. Refused for a build that has no Developer ID signature and
/// for an app that cannot be replaced where it is.
pub fn readiness() -> Result<(PathBuf, String), Problem> {
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|_| Problem::CannotUpdateItself)?;
    let bundle = bundle_of(&executable).ok_or(Problem::CannotUpdateItself)?;
    let team = developer_id_team(&bundle).ok_or(Problem::CannotUpdateItself)?;
    let bundle = install_target(&executable, &cannot_write)?;
    Ok((bundle, team))
}

/// A sibling of `bundle` with a hidden name: `.Cereal.app.<suffix>`.
fn sibling(bundle: &Path, suffix: &str) -> Option<PathBuf> {
    let name = bundle.file_name()?.to_str()?;
    Some(bundle.parent()?.join(format!(".{name}.{suffix}")))
}

fn remove_tree(path: &Path) {
    if path.symlink_metadata().is_ok() {
        let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
    }
}

/// How bundles are moved. Kept apart from the order they are moved in, so
/// the order can be tested against a move that fails.
pub struct Moves<'a> {
    pub rename: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
    /// A copy that keeps everything a signature covers.
    pub copy: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
}

fn copy_bundle(from: &Path, to: &Path) -> std::io::Result<()> {
    if succeeds(Command::new("/usr/bin/ditto").arg(from).arg(to)) {
        Ok(())
    } else {
        Err(std::io::Error::other("the bundle could not be copied"))
    }
}

/// Put `new_app` in the place of `bundle`.
///
/// The new bundle is first brought beside the old one (a rename on the same
/// volume, a copy from another), the old one is moved aside to a hidden
/// sibling, and the new one is renamed into place. If that last step fails
/// the old bundle is moved back, so the path never stays without an app.
/// Returns where the old bundle is; it is deleted at the next launch.
pub fn swap(new_app: &Path, bundle: &Path, moves: &Moves) -> Result<PathBuf, Problem> {
    let staged = sibling(bundle, "update-new").ok_or(Problem::Install)?;
    let aside = sibling(bundle, "update-old").ok_or(Problem::Install)?;
    remove_tree(&staged);
    remove_tree(&aside);
    if (moves.rename)(new_app, &staged).is_err() && (moves.copy)(new_app, &staged).is_err() {
        remove_tree(&staged);
        return Err(Problem::Install);
    }
    if (moves.rename)(bundle, &aside).is_err() {
        remove_tree(&staged);
        return Err(Problem::Install);
    }
    if (moves.rename)(&staged, bundle).is_err() {
        // Put the old app back where it was.
        let _ = (moves.rename)(&aside, bundle);
        remove_tree(&staged);
        return Err(Problem::Install);
    }
    Ok(aside)
}

/// Replace the app at `bundle` with the verified `new_app`.
pub fn install(new_app: &Path, bundle: &Path, team: &str) -> Result<(), Problem> {
    let moves = Moves {
        rename: &|from, to| std::fs::rename(from, to),
        copy: &copy_bundle,
    };
    let aside = swap(new_app, bundle, &moves)?;
    // What is now in place must still be the app that was verified, however
    // it got there. If it is not, the old one goes back.
    let intact = requirement(team).is_some_and(|requirement| {
        succeeds(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict", "--all-architectures"])
                .arg(format!("-R={requirement}"))
                .arg(bundle),
        )
    });
    if !intact {
        let refused = sibling(bundle, "update-new").ok_or(Problem::Install)?;
        remove_tree(&refused);
        if std::fs::rename(bundle, &refused).is_ok() {
            let _ = std::fs::rename(&aside, bundle);
            remove_tree(&refused);
        }
        return Err(Problem::Unverified);
    }
    Ok(())
}

/// Start the app at `bundle` once this process has exited. The helper is in
/// a process group of its own, so it outlives the app that started it. The
/// `CEREAL_*` settings this process was started with go to the new one,
/// except a session token, which is never written on a command line.
pub fn relaunch_after_exit(bundle: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    const SCRIPT: &str = r#"pid="$1"; app="$2"; shift 2
tries=0
while kill -0 "$pid" 2>/dev/null; do
  tries=$((tries + 1))
  [ "$tries" -gt 600 ] && exit 1
  sleep 0.2
done
exec /usr/bin/open -n "$@" "$app""#;
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", SCRIPT, "cereal-relaunch"])
        .arg(std::process::id().to_string())
        .arg(bundle);
    for (name, value) in std::env::vars() {
        if name.starts_with("CEREAL_") && name != "CEREAL_SERVER_TOKEN" {
            command.arg("--env").arg(format!("{name}={value}"));
        }
    }
    command
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

/// An update that was refused leaves nothing behind.
pub fn discard_download() {
    if let Some(directory) = work_directory() {
        remove_tree(&directory);
    }
}

static TIDYING: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// Remove what an earlier update left behind: the bundle that was replaced,
/// and the folder the update was downloaded in. Called once at launch.
pub fn tidy() {
    let tidying = std::thread::spawn(|| {
        if let Some(bundle) = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .ok()
            .and_then(|executable| bundle_of(&executable))
        {
            for suffix in ["update-old", "update-new"] {
                if let Some(path) = sibling(&bundle, suffix) {
                    remove_tree(&path);
                }
            }
        }
        if let Some(directory) = work_directory() {
            remove_tree(&directory);
        }
    });
    if let Ok(mut slot) = TIDYING.lock() {
        *slot = Some(tidying);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn manifest(mac: &str) -> Vec<u8> {
        format!(r#"{{"version":"0.2.0","mac":{mac},"iosAppStoreUrl":null}}"#).into_bytes()
    }

    fn mac(update: &str) -> String {
        format!(
            r#"{{"arch":"arm64","minimumSystem":"14.2","installer":{{"file":"Cereal-0.2.0-mac-arm64.dmg","url":"/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.dmg","size":10,"sha256":"{DIGEST}"}},"update":{update}}}"#
        )
    }

    fn update(file: &str, url: &str) -> String {
        format!(r#"{{"file":"{file}","url":"{url}","size":3,"sha256":"{DIGEST}"}}"#)
    }

    fn offer() -> Offer {
        Offer {
            version: "0.2.0".into(),
            path: "/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip".into(),
            size: 3,
            sha256: DIGEST.into(),
        }
    }

    /// A folder of this test's own under the build directory.
    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cereal-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn versions_compare_number_by_number() {
        assert_eq!(newer("0.1.10", "0.1.9"), Some(true));
        assert_eq!(newer("0.1.9", "0.1.10"), Some(false));
        assert_eq!(newer("0.1.0", "0.0.9"), Some(true));
        assert_eq!(newer("1.0.0", "0.99.99"), Some(true));
        assert_eq!(newer("0.1.0", "0.1.0"), Some(false));
        assert_eq!(newer("0.0.9", "0.1.0"), Some(false));
        // A shorter version is the same as one padded with zeros.
        assert_eq!(newer("1.0", "1.0.0"), Some(false));
        assert_eq!(newer("1.0.1", "1.0"), Some(true));
        // A suffix does not make a version later than its own number.
        assert_eq!(newer("0.1.0-beta.2", "0.1.0"), Some(false));
        assert_eq!(newer("0.2.0+build.7", "0.1.0"), Some(true));
        for invalid in ["", "abc", "1..2", "1.x", "-1.0", "1.2.3.4.5", "1.0000000000"] {
            assert_eq!(newer(invalid, "0.1.0"), None, "{invalid}");
            assert_eq!(newer("0.1.0", invalid), None, "{invalid}");
        }
    }

    #[test]
    fn a_manifest_is_read_with_or_without_an_update() {
        let name = "Cereal-0.2.0-mac-arm64.zip";
        let full = manifest(&mac(&update(name, &format!("/api/v1/downloads/files/{name}"))));
        assert_eq!(
            Manifest::read(&full, "arm64"),
            Ok(Manifest {
                version: "0.2.0".into(),
                update: Some(offer()),
            })
        );
        // Only the installer was published, or nothing was.
        for body in [manifest(&mac("null")), manifest("null")] {
            assert_eq!(
                Manifest::read(&body, "arm64"),
                Ok(Manifest {
                    version: "0.2.0".into(),
                    update: None,
                })
            );
        }
        // A build for another processor is not one for this Mac.
        assert_eq!(Manifest::read(&full, "x86_64").unwrap().update, None);
        // Fields this app does not know are ignored.
        let extended = br#"{"version":"0.2.0","channel":"stable","mac":{"arch":"arm64","notes":["a"],"update":{"file":"Cereal-0.2.0-mac-arm64.zip","url":"/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","signature":"x"}},"windows":{}}"#;
        assert_eq!(Manifest::read(extended, "arm64").unwrap().update, Some(offer()));
        for broken in [&b"not json"[..], b"{}", br#"{"version":"latest","mac":null}"#, br#"{"version":7}"#] {
            assert_eq!(Manifest::read(broken, "arm64"), Err(Problem::Manifest));
        }
    }

    #[test]
    fn an_archive_is_only_ever_a_file_of_the_same_servers_downloads() {
        let name = "Cereal-0.2.0-mac-arm64.zip";
        let hostile = [
            (name, "https://evil.example/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip"),
            (name, "//evil.example/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip"),
            (name, "/api/v1/downloads/files/../../../etc/Cereal-0.2.0-mac-arm64.zip"),
            (name, "/api/v1/downloads/files/../Cereal-0.2.0-mac-arm64.zip"),
            (name, "/api/v1/downloads/files/other.zip"),
            (name, "/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip?redirect=https://evil.example"),
            (name, "/api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip#x"),
            (name, "/api/v1/notes/Cereal-0.2.0-mac-arm64.zip"),
            (name, "api/v1/downloads/files/Cereal-0.2.0-mac-arm64.zip"),
            (name, "/api/v1/downloads/files/%2e%2e/Cereal-0.2.0-mac-arm64.zip"),
            ("../x.zip", "/api/v1/downloads/files/../x.zip"),
            ("a/b.zip", "/api/v1/downloads/files/a/b.zip"),
            ("..zip", "/api/v1/downloads/files/..zip"),
            (".hidden.zip", "/api/v1/downloads/files/.hidden.zip"),
            ("Cereal.dmg", "/api/v1/downloads/files/Cereal.dmg"),
            ("", "/api/v1/downloads/files/"),
        ];
        for (file, url) in hostile {
            assert_eq!(
                Manifest::read(&manifest(&mac(&update(file, url))), "arm64"),
                Err(Problem::Manifest),
                "{file} at {url}"
            );
        }
        // A size or digest that is not one is refused as well.
        for update in [
            format!(r#"{{"file":"{name}","url":"/api/v1/downloads/files/{name}","size":0,"sha256":"{DIGEST}"}}"#),
            format!(r#"{{"file":"{name}","url":"/api/v1/downloads/files/{name}","size":-4,"sha256":"{DIGEST}"}}"#),
            format!(r#"{{"file":"{name}","url":"/api/v1/downloads/files/{name}","size":"3","sha256":"{DIGEST}"}}"#),
            format!(r#"{{"file":"{name}","url":"/api/v1/downloads/files/{name}","size":3,"sha256":"abc"}}"#),
            format!(r#"{{"file":"{name}","url":"/api/v1/downloads/files/{name}","size":3,"sha256":"{}"}}"#, DIGEST.to_uppercase()),
            format!(r#"{{"file":"{name}","url":"/api/v1/downloads/files/{name}","size":3}}"#),
        ] {
            assert_eq!(
                Manifest::read(&manifest(&mac(&update)), "arm64"),
                Err(Problem::Manifest),
                "{update}"
            );
        }
    }

    #[test]
    fn only_a_later_version_is_an_update() {
        let with = |version: &str, update: Option<Offer>| Manifest {
            version: version.into(),
            update,
        };
        assert_eq!(
            evaluate(with("0.2.0", Some(offer())), "0.1.0"),
            Ok(Check::Available(offer()))
        );
        // The same version and an earlier one are both "up to date",
        // whether or not the server has an archive: never a downgrade.
        assert_eq!(evaluate(with("0.1.0", Some(offer())), "0.1.0"), Ok(Check::UpToDate));
        assert_eq!(evaluate(with("0.0.9", Some(offer())), "0.1.0"), Ok(Check::UpToDate));
        assert_eq!(evaluate(with("0.1.0", None), "0.1.0"), Ok(Check::UpToDate));
        // A later server with nothing published for this Mac.
        assert_eq!(evaluate(with("0.2.0", None), "0.1.0"), Err(Problem::NoBuild));
    }

    #[test]
    fn an_archive_must_have_the_advertised_size_and_digest() {
        let directory = scratch("archive");
        let path = directory.join("update.zip");
        // "abc" is three bytes and hashes to DIGEST.
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(check_archive(&path, &offer()), Ok(()));
        // The same size, other bytes.
        std::fs::write(&path, b"abd").unwrap();
        assert_eq!(check_archive(&path, &offer()), Err(Problem::Unverified));
        // The right bytes and more.
        std::fs::write(&path, b"abcd").unwrap();
        assert_eq!(check_archive(&path, &offer()), Err(Problem::Unverified));
        // A download that stopped short.
        std::fs::write(&path, b"ab").unwrap();
        assert_eq!(check_archive(&path, &offer()), Err(Problem::Download));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(check_archive(&path, &offer()), Err(Problem::Download));
        // The right size and a digest that is someone else's.
        std::fs::write(&path, b"abc").unwrap();
        let other = Offer {
            sha256: "0".repeat(64),
            ..offer()
        };
        assert_eq!(check_archive(&path, &other), Err(Problem::Unverified));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn the_unpacked_app_must_be_the_advertised_and_a_later_version() {
        assert_eq!(check_bundle_version(Some("0.2.0"), &offer(), "0.1.0"), Ok(()));
        // An app that claims another version than the server advertised.
        assert_eq!(
            check_bundle_version(Some("0.0.5"), &offer(), "0.1.0"),
            Err(Problem::Unverified)
        );
        assert_eq!(
            check_bundle_version(Some("0.3.0"), &offer(), "0.1.0"),
            Err(Problem::Unverified)
        );
        // The advertised version, but not later than the one running.
        assert_eq!(
            check_bundle_version(Some("0.2.0"), &offer(), "0.2.0"),
            Err(Problem::Unverified)
        );
        assert_eq!(
            check_bundle_version(Some("0.2.0"), &offer(), "0.3.0"),
            Err(Problem::Unverified)
        );
        assert_eq!(check_bundle_version(None, &offer(), "0.1.0"), Err(Problem::Unverified));
    }

    #[test]
    fn an_archive_holds_only_the_app_and_no_links() {
        let names = "Cereal.app/\nCereal.app/Contents/\nCereal.app/Contents/MacOS/cereal\nCereal.app/Contents/._Info.plist\n";
        let listing = "Archive:  update.zip\nZip file size: 9212842 bytes, number of entries: 4\ndrwxr-xr-x  2.1 unx        0 bx stor 26-Nov-14 19:17 Cereal.app/\n-rwxr-xr-x  2.1 unx 17109392 bX defN 26-Nov-14 19:17 Cereal.app/Contents/MacOS/cereal\n4 files, 100 bytes uncompressed, 50 bytes compressed:  50.0%\n";
        assert!(archive_entries_allowed(names, listing));
        for hostile in [
            "Cereal.app/\n../evil\n",
            "Cereal.app/\nCereal.app/../../Library/LaunchAgents/evil.plist\n",
            "Cereal.app/\n/etc/evil\n",
            "Cereal.app/\nOther.app/Contents/MacOS/x\n",
            "Cereal.app/\n__MACOSX/Cereal.app/._x\n",
            "Cereal.app\n",
            "evil\n",
            "",
        ] {
            assert!(!archive_entries_allowed(hostile, listing), "{hostile:?}");
        }
        // A link inside the app that later entries could be written through.
        let link = "drwxr-xr-x  2.1 unx        0 bx stor 26-Nov-14 19:17 Cereal.app/\nlrwxr-xr-x  2.1 unx       24 bx stor 26-Nov-14 19:17 Cereal.app/Contents/Library\n";
        assert!(!archive_entries_allowed(names, link));
    }

    #[test]
    fn the_requirement_names_the_team_and_refuses_anything_that_is_not_one() {
        let requirement = requirement("LZWN2FF7YB").unwrap();
        assert_eq!(
            requirement,
            "anchor apple generic and identifier \"dev.cereal.desktop\" and certificate leaf[subject.OU] = \"LZWN2FF7YB\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
        );
        for team in ["", "lzwn2ff7yb", "LZWN2FF7Y", "LZWN2FF7YB\" or anchor apple", "LZWN2FF7YBX"] {
            assert_eq!(super::requirement(team), None, "{team}");
        }
    }

    #[test]
    fn where_the_app_is_decides_whether_it_can_be_replaced() {
        let writable = |_: &Path| false;
        let read_only = |_: &Path| true;
        let normal = Path::new("/Applications/Cereal.app/Contents/MacOS/cereal");
        assert_eq!(
            install_target(normal, &writable),
            Ok(PathBuf::from("/Applications/Cereal.app"))
        );
        // Whatever the bundle is called, it is replaced under that name.
        assert_eq!(
            install_target(Path::new("/Users/me/Apps/Cereal 2.app/Contents/MacOS/cereal"), &writable),
            Ok(PathBuf::from("/Users/me/Apps/Cereal 2.app"))
        );
        let translocated = Path::new(
            "/private/var/folders/ab/cd/T/AppTranslocation/6B1F0A52-7C55-4A62-9A0E-0D5B2E9B3A11/d/Cereal.app/Contents/MacOS/cereal",
        );
        assert_eq!(
            install_target(translocated, &writable),
            Err(Problem::MoveToApplications)
        );
        // A disk image, or any other place nothing can be written.
        assert_eq!(
            install_target(Path::new("/Volumes/Cereal/Cereal.app/Contents/MacOS/cereal"), &read_only),
            Err(Problem::MoveToApplications)
        );
        // Not in a bundle at all: a build run from the build directory.
        for bare in ["/Users/me/cereal/target/debug/cereal", "/usr/local/bin/cereal", "/Cereal.app/cereal"] {
            assert_eq!(
                install_target(Path::new(bare), &writable),
                Err(Problem::CannotUpdateItself),
                "{bare}"
            );
        }
        // The real probe: a folder that exists can be written, one that
        // does not cannot.
        let directory = scratch("probe");
        assert!(!cannot_write(&directory));
        assert!(cannot_write(&directory.join("missing")));
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(directory);
    }

    /// A bundle with one file in it that says which it is.
    fn bundle(path: &Path, mark: &str) {
        std::fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
        std::fs::write(path.join("Contents/MacOS/cereal"), mark).unwrap();
    }

    fn mark(path: &Path) -> Option<String> {
        std::fs::read_to_string(path.join("Contents/MacOS/cereal")).ok()
    }

    fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
        bundle(to, &mark(from).ok_or_else(|| std::io::Error::other("nothing to copy"))?);
        Ok(())
    }

    #[test]
    fn a_swap_puts_the_new_app_in_place_and_keeps_the_old_one_aside() {
        let root = scratch("swap");
        let (installed, unpacked) = (root.join("Apps/Cereal.app"), root.join("work/Cereal.app"));
        bundle(&installed, "old");
        bundle(&unpacked, "new");
        // Something a failed earlier update left behind is cleared first.
        bundle(&root.join("Apps/.Cereal.app.update-old"), "stale");
        let aside = swap(&unpacked, &installed, &Moves { rename: &rename, copy: &copy }).unwrap();
        assert_eq!(mark(&installed).as_deref(), Some("new"));
        assert_eq!(aside, root.join("Apps/.Cereal.app.update-old"));
        assert_eq!(mark(&aside).as_deref(), Some("old"));
        assert!(!unpacked.exists());
        assert!(!root.join("Apps/.Cereal.app.update-new").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_new_app_on_another_volume_is_copied_beside_the_old_one_first() {
        let root = scratch("copy");
        let (installed, unpacked) = (root.join("Apps/Cereal.app"), root.join("work/Cereal.app"));
        bundle(&installed, "old");
        bundle(&unpacked, "new");
        // The first rename, from the work folder, fails as it does across
        // volumes; the renames beside the installed app work.
        let calls = Cell::new(0);
        let across = |from: &Path, to: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Err(std::io::Error::other("cross-device link"))
            } else {
                std::fs::rename(from, to)
            }
        };
        let aside = swap(&unpacked, &installed, &Moves { rename: &across, copy: &copy }).unwrap();
        assert_eq!(mark(&installed).as_deref(), Some("new"));
        assert_eq!(mark(&aside).as_deref(), Some("old"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_swap_that_fails_at_any_step_leaves_the_old_app_in_place() {
        // Fail the first, second or third move; the copy fails too, so the
        // first step has no way through.
        for failing in 1..=3 {
            let root = scratch(&format!("rollback-{failing}"));
            let (installed, unpacked) = (root.join("Apps/Cereal.app"), root.join("work/Cereal.app"));
            bundle(&installed, "old");
            bundle(&unpacked, "new");
            let calls = Cell::new(0);
            let breaking = |from: &Path, to: &Path| {
                calls.set(calls.get() + 1);
                if calls.get() == failing {
                    Err(std::io::Error::other("no"))
                } else {
                    std::fs::rename(from, to)
                }
            };
            let no_copy = |_: &Path, _: &Path| Err(std::io::Error::other("no"));
            let result = swap(&unpacked, &installed, &Moves { rename: &breaking, copy: &no_copy });
            assert_eq!(result, Err(Problem::Install), "step {failing}");
            assert_eq!(mark(&installed).as_deref(), Some("old"), "step {failing}");
            // Nothing half-moved is left beside the app.
            let left: Vec<_> = std::fs::read_dir(root.join("Apps"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(left, ["Cereal.app"], "step {failing}");
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_plist_string_is_read_from_the_bundle() {
        let root = scratch("plist");
        let app = root.join("Cereal.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n    <key>CFBundleShortVersionString</key><string>0.0.9</string>\n    <key>CFBundleVersion</key>\n    <string>1</string>\n</dict>\n</plist>\n",
        )
        .unwrap();
        assert_eq!(bundle_string(&app, "CFBundleShortVersionString").as_deref(), Some("0.0.9"));
        assert_eq!(bundle_string(&app, "CFBundleVersion").as_deref(), Some("1"));
        assert_eq!(bundle_string(&app, "LSMinimumSystemVersion"), None);
        assert_eq!(bundle_string(&root.join("Missing.app"), "CFBundleVersion"), None);
        // A test binary is not in a bundle: it reports the compiled version.
        assert_eq!(current_version(), env!("CARGO_PKG_VERSION"));
        let _ = std::fs::remove_dir_all(root);
    }
}
