//! Looks for a newer Glint among the GitHub releases, downloads it and puts it in place.
//!
//! The crate is UI-free. The app asks [`check`] what is new, [`download`]s the file the release
//! has for the way this copy was installed (see [`Method`]), and calls [`install`], after which it
//! quits: the new copy is already starting.
//!
//! Nothing is installed that GitHub did not publish a SHA-256 for: the download is hashed as it
//! arrives and thrown away when the hash differs.

mod version;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub use version::Version;

/// The repository releases are read from.
const RELEASES: &str = "https://api.github.com/repos/orphytype/glint/releases?per_page=30";
/// Every file is fetched from here, and from nowhere else.
const DOWNLOADS: &str = "https://github.com/orphytype/glint/releases/download/";
/// More than this is no Glint download, whatever the release says about its size.
const MAX_DOWNLOAD: u64 = 400 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),
    #[error("unexpected answer: {0}")]
    Unexpected(String),
    #[error("the download does not match its checksum")]
    Checksum,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("this copy of Glint cannot update itself")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, Error>;

/// How this copy of Glint was installed, which decides what an update is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Windows, installed with the setup program: the new setup runs quietly over it.
    WindowsInstaller,
    /// Windows, the portable zip (a `glint.portable` file beside the exe): the exe is swapped.
    WindowsPortable,
    /// Linux AppImage: the file is replaced.
    AppImage,
    /// Anything else (a .deb or .rpm, the tarball, a build from source): the system's own tools
    /// or the user update it, and Glint only says that a new version exists.
    Manual,
}

impl Method {
    pub fn detect() -> Method {
        #[cfg(windows)]
        {
            let portable = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("glint.portable").is_file())).unwrap_or(false);
            if portable { Method::WindowsPortable } else { Method::WindowsInstaller }
        }
        #[cfg(target_os = "linux")]
        {
            match appimage_path() {
                Some(_) => Method::AppImage,
                None => Method::Manual,
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Method::Manual
        }
    }

    /// Whether Glint can put the new version in place itself.
    pub fn can_install(self) -> bool {
        self != Method::Manual
    }

    /// The end of the name of the release file that fits this method.
    fn asset_suffix(self) -> Option<&'static str> {
        match self {
            Method::WindowsInstaller => Some("-windows-x64-setup.exe"),
            Method::WindowsPortable => Some("-windows-x64-portable.zip"),
            Method::AppImage => Some("-x86_64.AppImage"),
            Method::Manual => None,
        }
    }
}

/// A file of a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256, as GitHub states it for the file.
    pub sha256: String,
}

/// A release newer than the running version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// `0.2.0`, without the `v` of the tag.
    pub version: String,
    /// The release's page on GitHub, for the notes and for a manual download.
    pub page_url: String,
    /// Plain-text release notes from the GitHub release body.
    pub notes: String,
    /// The file for this install, when the release has one that can be trusted.
    pub asset: Option<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

fn http() -> Result<reqwest::Client> {
    // rustls needs its crypto chosen once per process (Windows uses SChannel instead).
    #[cfg(not(windows))]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Glint/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        // Never follow a redirect off https.
        .redirect(reqwest::redirect::Policy::custom(|attempt| if attempt.url().scheme() == "https" && attempt.previous().len() < 8 { attempt.follow() } else { attempt.stop() }))
        .build()?)
}

/// The newest release that is newer than `current`, if there is one. A pre-release is offered only
/// to someone already running a pre-release.
pub async fn check(current: &str, method: Method) -> Result<Option<Release>> {
    let current = Version::parse(current).ok_or_else(|| Error::Unexpected(format!("own version {current:?}")))?;
    let list: Vec<ApiRelease> =
        http()?.get(RELEASES).header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28").send().await?.error_for_status()?.json().await?;
    Ok(newest(list, &current, method))
}

fn newest(list: Vec<ApiRelease>, current: &Version, method: Method) -> Option<Release> {
    list.into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| Version::parse(&r.tag_name).map(|v| (v, r)))
        .filter(|(v, _)| *v > *current && (current.is_pre() || !v.is_pre()))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, r)| {
            let asset = method.asset_suffix().and_then(|suffix| r.assets.iter().find(|a| a.name.ends_with(suffix))).and_then(trusted);
            Release { version: r.tag_name.trim_start_matches(['v', 'V']).to_owned(), page_url: r.html_url, notes: clean_notes(&r.body), asset }
        })
}

/// Turns a GitHub Markdown release body into concise plain text for the update popover, stopping
/// before the download-file table section.
fn clean_notes(body: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for raw in body.lines() {
        let trimmed = raw.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("## which file") || lower.starts_with("### which file") || lower.starts_with("which file to download") {
            break;
        }
        if trimmed.is_empty() || trimmed.starts_with("---") || trimmed.starts_with("|") {
            continue;
        }
        let stripped = trimmed.trim_start_matches('#').trim();
        let plain = if let Some(rest) = stripped.strip_prefix("- ").or_else(|| stripped.strip_prefix("* ")) { format!("• {}", strip_md(rest)) } else { strip_md(stripped) };
        if !plain.is_empty() {
            lines.push(plain);
        }
        if lines.len() >= 12 {
            break;
        }
    }
    lines.join("\n")
}

fn strip_md(s: &str) -> String {
    s.replace("**", "").replace('`', "").trim().to_owned()
}

/// The asset, if it is from this project's releases and GitHub gave its checksum.
fn trusted(a: &ApiAsset) -> Option<Asset> {
    let hash = a.digest.as_deref()?.strip_prefix("sha256:")?.to_ascii_lowercase();
    let name_ok = !a.name.is_empty() && !a.name.contains(['/', '\\']);
    (a.browser_download_url.starts_with(DOWNLOADS) && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && name_ok).then(|| Asset {
        name: a.name.clone(),
        url: a.browser_download_url.clone(),
        size: a.size,
        sha256: hash,
    })
}

/// Where [`download`] should put `asset` for this method. The AppImage goes beside the one that
/// runs, so that replacing it is a rename on the same disk.
pub fn download_path(method: Method, asset: &Asset) -> Result<PathBuf> {
    match method {
        Method::WindowsInstaller | Method::WindowsPortable => {
            let dir = scratch_dir();
            std::fs::create_dir_all(&dir)?;
            Ok(dir.join(&asset.name))
        }
        #[cfg(target_os = "linux")]
        Method::AppImage => {
            let mut path = appimage_path().ok_or(Error::Unsupported)?.into_os_string();
            path.push(".update");
            Ok(PathBuf::from(path))
        }
        _ => Err(Error::Unsupported),
    }
}

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join("glint-update")
}

/// Fetches `asset` into `dest`, calling `progress(received, total)` as it arrives, and keeps the
/// file only when its SHA-256 is the one GitHub stated.
pub async fn download(asset: &Asset, dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<()> {
    if !asset.url.starts_with(DOWNLOADS) {
        return Err(Error::Unsupported);
    }
    let mut resp = http()?.get(&asset.url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(asset.size);
    let limit = if asset.size > 0 { asset.size.min(MAX_DOWNLOAD) } else { MAX_DOWNLOAD };
    let part = with_suffix(dest, ".part");
    let result = async {
        let mut out = std::fs::File::create(&part)?;
        let mut hash = Sha256::new();
        let mut got = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            got += chunk.len() as u64;
            if got > limit {
                return Err(Error::Unexpected("the file is larger than the release says".into()));
            }
            hash.update(&chunk);
            out.write_all(&chunk)?;
            progress(got, total);
        }
        out.flush()?;
        drop(out);
        if hex(&hash.finalize()) != asset.sha256 {
            return Err(Error::Checksum);
        }
        std::fs::rename(&part, dest)?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Puts the downloaded `file` in place and starts the new copy. The caller quits right after, so
/// that the new copy has the audio device, the tray and the files to itself.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(unused_variables))]
pub fn install(method: Method, file: &Path) -> Result<()> {
    match method {
        #[cfg(windows)]
        Method::WindowsInstaller => {
            // Setup closes what is left of this copy by itself and starts Glint again when it is done.
            windows::detached(std::process::Command::new(file).args(["/SILENT", "/NORESTART", "/CLOSEAPPLICATIONS"]))
        }
        #[cfg(windows)]
        Method::WindowsPortable => windows::swap_portable(file),
        #[cfg(target_os = "linux")]
        Method::AppImage => linux::replace_appimage(file),
        _ => Err(Error::Unsupported),
    }
}

/// Removes what an update leaves behind: the old exe of a portable copy and the downloads. Cheap;
/// call it when Glint starts.
pub fn cleanup() {
    let _ = std::fs::remove_dir_all(scratch_dir());
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(with_suffix(&exe, ".old"));
        let _ = std::fs::remove_file(with_suffix(&exe, ".new"));
    }
    #[cfg(target_os = "linux")]
    if let Some(path) = appimage_path() {
        let _ = std::fs::remove_file(with_suffix(&path, ".update"));
        let _ = std::fs::remove_file(with_suffix(&path, ".update.part"));
    }
}

#[cfg(target_os = "linux")]
fn appimage_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file())
}

#[cfg(windows)]
mod windows {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    use super::{Error, Result, with_suffix};

    /// `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`: the child outlives this process.
    const DETACHED: u32 = 0x0000_0008 | 0x0000_0200;
    /// More than this is not a Glint exe.
    const MAX_EXE: u64 = 300 * 1024 * 1024;

    pub fn detached(cmd: &mut Command) -> Result<()> {
        cmd.creation_flags(DETACHED).spawn()?;
        Ok(())
    }

    /// Takes `glint.exe` out of the portable zip and swaps it for the running one. Windows lets a
    /// running exe be renamed, not overwritten, so the old one is moved aside (and removed by
    /// `cleanup` at the next start).
    pub fn swap_portable(zip_file: &Path) -> Result<()> {
        let exe = std::env::current_exe()?;
        let (new, old) = (with_suffix(&exe, ".new"), with_suffix(&exe, ".old"));
        let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_file)?).map_err(|e| Error::Unexpected(e.to_string()))?;
        let mut found = None;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).map_err(|e| Error::Unexpected(e.to_string()))?;
            if entry.enclosed_name().and_then(|p| p.file_name().map(|n| n == "glint.exe")).unwrap_or(false) {
                found = Some(i);
                break;
            }
        }
        let mut entry = archive.by_index(found.ok_or_else(|| Error::Unexpected("no glint.exe in the zip".into()))?).map_err(|e| Error::Unexpected(e.to_string()))?;
        let mut out = std::fs::File::create(&new)?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_EXE + 1), &mut out);
        drop(out);
        if copied.as_ref().map_or(true, |n| *n > MAX_EXE || *n == 0) {
            let _ = std::fs::remove_file(&new);
            return Err(copied.map(|_| Error::Unexpected("the exe in the zip is not usable".into())).unwrap_or_else(Error::Io));
        }
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)?;
        if let Err(e) = std::fs::rename(&new, &exe) {
            let _ = std::fs::rename(&old, &exe);
            let _ = std::fs::remove_file(&new);
            return Err(e.into());
        }
        detached(&mut Command::new(&exe))
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::{Error, Result, appimage_path};

    /// Moves the downloaded AppImage over the one that is running and starts it.
    pub fn replace_appimage(file: &Path) -> Result<()> {
        let target = appimage_path().ok_or(Error::Unsupported)?;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))?;
        // A rename keeps the running copy intact: it goes on reading the old file until it exits.
        std::fs::rename(file, &target)?;
        Command::new(&target)
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .env_remove("ARGV0")
            .env_remove("OWD")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(())
    }
}
